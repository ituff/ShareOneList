use std::sync::Arc;

use chrono::{DateTime, Utc};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::Deserialize;
use tauri::State;
use tokio::sync::Mutex;

use crate::auth::cloud_config::CloudEnvironment;
use crate::auth::AuthModule;
use crate::errors::AppError;
use crate::graph::GraphClient;
use crate::models::{
    Drive, DriveItem, DriveQuota, MeetingRecording, RecordingSource, ShareOptions, Site,
    TranscriptExport,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a cloud environment string ("global" or "china") into the enum.
fn parse_cloud_env(cloud_env: &str) -> Result<CloudEnvironment, AppError> {
    match cloud_env.to_lowercase().as_str() {
        "global" => Ok(CloudEnvironment::Global),
        "china" => Ok(CloudEnvironment::China),
        _ => Err(AppError::Validation {
            message: format!(
                "Invalid cloud environment '{}'. Expected 'global' or 'china'.",
                cloud_env
            ),
            field: "cloud_env".to_string(),
        }),
    }
}

/// Graph API collection response wrapper (`{ "value": [...] }`).
#[derive(Debug, Deserialize)]
struct GraphCollection<T> {
    value: Vec<T>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

/// Raw drive item as returned by Graph API (different field names from our model).
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawDriveItem {
    id: Option<String>,
    name: Option<String>,
    size: Option<u64>,
    #[serde(rename = "lastModifiedDateTime")]
    last_modified_date_time: Option<String>,
    folder: Option<serde_json::Value>,
    file: Option<RawFile>,
    #[serde(rename = "webUrl")]
    web_url: Option<String>,
    #[serde(rename = "parentReference")]
    parent_reference: Option<RawParentReference>,
    #[serde(rename = "@microsoft.graph.downloadUrl")]
    download_url: Option<String>,
    #[serde(rename = "createdDateTime")]
    created_date_time: Option<String>,
    #[serde(rename = "remoteItem")]
    remote_item: Option<RawRemoteItem>,
    package: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawRemoteItem {
    id: Option<String>,
    name: Option<String>,
    folder: Option<serde_json::Value>,
    package: Option<serde_json::Value>,
    #[serde(rename = "parentReference")]
    parent_reference: Option<RawParentReference>,
}

#[derive(Debug, Deserialize)]
struct RawFile {
    #[serde(rename = "mimeType")]
    mime_type: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct RawParentReference {
    #[serde(rename = "driveId")]
    drive_id: Option<String>,
    id: Option<String>,
    path: Option<String>,
    name: Option<String>,
}

impl From<RawDriveItem> for DriveItem {
    fn from(raw: RawDriveItem) -> Self {
        // sharedWithMe returns remote items (files shared from other people's
        // drives) with their real identity inside `remoteItem`; the top-level
        // wrapper carries only transient sharing metadata.  Fall back to the
        // remote payload when the top-level parentReference is missing (the
        // real location lives on the remote drive, not the shim).
        let is_remote = raw.parent_reference.is_none() && raw.remote_item.is_some();

        let (id, name, parent_ref, size, download_url, mime_type, web_url, is_folder, created) =
            if is_remote {
                let ri = raw.remote_item.as_ref().unwrap();
                (
                    ri.id.clone().unwrap_or_default(),
                    ri.name.clone().unwrap_or_default(),
                    ri.parent_reference.clone().map(|pr| crate::models::ParentReference {
                        drive_id: pr.drive_id.unwrap_or_default(),
                        id: pr.id.unwrap_or_default(),
                        path: pr.path,
                        name: pr.name,
                    }),
                    raw.size,
                    raw.download_url,
                    raw.file.and_then(|f| f.mime_type),
                    raw.web_url,
                    raw.folder.is_some() || ri.folder.is_some() || ri.package.is_some(),
                    raw.created_date_time,
                )
            } else {
                (
                    raw.id.unwrap_or_default(),
                    raw.name.unwrap_or_default(),
                    raw.parent_reference.map(|pr| crate::models::ParentReference {
                        drive_id: pr.drive_id.unwrap_or_default(),
                        id: pr.id.unwrap_or_default(),
                        path: pr.path,
                        name: pr.name,
                    }),
                    raw.size,
                    raw.download_url,
                    raw.file.and_then(|f| f.mime_type),
                    raw.web_url,
                    raw.folder.is_some() || raw.package.is_some(),
                    raw.created_date_time,
                )
            };

        DriveItem {
            id,
            name,
            size,
            last_modified: raw.last_modified_date_time.unwrap_or_default(),
            is_folder,
            mime_type,
            web_url,
            parent_reference: parent_ref,
            download_url,
            created_date_time: created,
        }
    }
}

const DRIVE_ITEM_SELECT: &str =
    "id,name,size,lastModifiedDateTime,folder,file,remoteItem,package,webUrl,parentReference,@microsoft.graph.downloadUrl,createdDateTime";
const SIZE_CHILD_SELECT: &str = "id,name,size,folder,remoteItem,package";

async fn sum_folder_size(
    client: &GraphClient,
    token: &str,
    drive_id: &str,
    folder_id: &str,
    depth: u32,
) -> Result<u64, AppError> {
    if depth >= 32 {
        return Ok(0);
    }

    let base = client.base_url();
    let mut total = 0u64;
    let mut url = format!(
        "{}/drives/{}/items/{}/children?$top=200&$select={}",
        base, drive_id, folder_id, SIZE_CHILD_SELECT
    );

    loop {
        let current_url = url.clone();
        let response = client
            .request_with_retry(token, |http, tkn| http.get(&current_url).bearer_auth(tkn))
            .await?;

        let collection: GraphCollection<RawDriveItem> =
            response.json().await.map_err(|e| AppError::GraphApi {
                message: format!("Failed to parse folder size response: {}", e),
                status_code: 0,
            })?;

        for raw in collection.value {
            let item = DriveItem::from(raw);
            if item.is_folder && !item.id.is_empty() {
                total = total.saturating_add(
                    Box::pin(sum_folder_size(
                        client,
                        token,
                        drive_id,
                        &item.id,
                        depth + 1,
                    ))
                    .await?,
                );
            } else {
                total = total.saturating_add(item.size.unwrap_or(0));
            }
        }

        match collection.next_link {
            Some(next) => url = next,
            None => break,
        }
    }

    Ok(total)
}

/// Raw drive as returned by Graph API.
#[derive(Debug, Deserialize)]
struct RawDrive {
    id: Option<String>,
    name: Option<String>,
    #[serde(rename = "driveType")]
    drive_type: Option<String>,
    quota: Option<RawQuota>,
}

#[derive(Debug, Deserialize)]
struct RawQuota {
    total: Option<u64>,
    used: Option<u64>,
    remaining: Option<u64>,
}

impl From<RawDrive> for Drive {
    fn from(raw: RawDrive) -> Self {
        Drive {
            id: raw.id.unwrap_or_default(),
            name: raw.name.unwrap_or_default(),
            drive_type: raw.drive_type.unwrap_or_default(),
            quota: raw.quota.map(|q| DriveQuota {
                total: q.total.unwrap_or(0),
                used: q.used.unwrap_or(0),
                remaining: q.remaining.unwrap_or(0),
            }),
        }
    }
}

/// Raw site as returned by Graph API.
#[derive(Debug, Deserialize)]
struct RawSite {
    id: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    name: Option<String>,
    #[serde(rename = "webUrl")]
    web_url: Option<String>,
}

impl From<RawSite> for Site {
    fn from(raw: RawSite) -> Self {
        Site {
            id: raw.id.unwrap_or_default(),
            display_name: raw.display_name.or(raw.name).unwrap_or_default(),
            web_url: raw.web_url.unwrap_or_default(),
        }
    }
}

/// Add a site to the list, skipping duplicate IDs.
fn push_unique_site(sites: &mut Vec<Site>, site: Site) {
    if !site.id.is_empty() && sites.iter().any(|s| s.id == site.id) {
        return;
    }
    sites.push(site);
}

/// Fetch a Graph site collection response, returning the parsed sites.
async fn fetch_site_collection(
    client: &GraphClient,
    token: &str,
    url: &str,
) -> Result<Vec<Site>, AppError> {
    let response = client
        .request_with_retry(token, |http, tkn| http.get(url).bearer_auth(tkn))
        .await?;

    let collection: GraphCollection<RawSite> =
        response.json().await.map_err(|e| AppError::GraphApi {
            message: format!("Failed to parse sites response: {}", e),
            status_code: 0,
        })?;

    Ok(collection.value.into_iter().map(Site::from).collect())
}

/// Share link creation response.
#[derive(Debug, Deserialize)]
struct ShareLinkResponse {
    link: Option<ShareLinkValue>,
}

#[derive(Debug, Deserialize)]
struct ShareLinkValue {
    #[serde(rename = "webUrl")]
    web_url: Option<String>,
}

/// Preview response.
#[derive(Debug, Deserialize)]
struct PreviewResponse {
    #[serde(rename = "getUrl")]
    get_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ThumbnailSetResponse {
    value: Vec<ThumbnailSet>,
}

#[derive(Debug, Deserialize)]
struct ThumbnailSet {
    small: Option<Thumbnail>,
    medium: Option<Thumbnail>,
    large: Option<Thumbnail>,
}

#[derive(Debug, Deserialize)]
struct Thumbnail {
    url: Option<String>,
}

/// Group object from /me/memberOf (for China SharePoint discovery).
#[derive(Debug, Deserialize)]
struct DirectoryObject {
    #[serde(rename = "@odata.type")]
    odata_type: Option<String>,
    id: Option<String>,
}

/// Whether a Graph parent path (e.g. `/drive/root:/Recordings/sub`) points
/// into a recordings folder, matching localized aliases case-insensitively.
#[allow(dead_code)]
fn path_points_to_recordings_folder(path: Option<&String>) -> bool {
    let Some(path) = path else {
        return false;
    };
    path.split('/').any(|segment| {
        !segment.is_empty()
            && RECORDINGS_FOLDER_ALIASES
                .iter()
                .any(|alias| segment.eq_ignore_ascii_case(alias))
    })
}

/// Collect .mp4 files shared with the user via POST /search/query. Microsoft
/// Search only returns items the signed-in user can access, mirroring what the
/// OneDrive web UI shows; hits on the user's own drive are filtered out so the
/// result is strictly the "shared with me" set (own recordings come from the
/// Recordings folder source).
/// This replaces the deprecated /me/drive/sharedWithMe endpoint which returns
/// far fewer items than the OneDrive web UI.
const RECORDING_SEARCH_QUERY: &str = "filetype:mp4";
const SEARCH_PAGE_SIZE: usize = 50;
const SEARCH_MAX_PAGES: usize = 4;

async fn collect_search_recordings(
    client: &GraphClient,
    token: &str,
    own_drive_id: &str,
) -> Vec<MeetingRecording> {
    let base = client.base_url();
    let url = format!("{}/search/query", base);
    let mut recordings = Vec::new();

    {
        let mut from = 0usize;
        for _page in 0..SEARCH_MAX_PAGES {
            let body = serde_json::json!({
                "requests": [{
                    "entityTypes": ["driveItem"],
                    "query": { "queryString": RECORDING_SEARCH_QUERY },
                    "from": from,
                    "size": SEARCH_PAGE_SIZE
                }]
            });

            let response = match client
                .request_with_retry(token, |http, tkn| http.post(&url).bearer_auth(tkn).json(&body))
                .await
            {
                Ok(response) => response,
                Err(e) => {
                    eprintln!(
                        "[recordings] Search source query '{}' failed: {}",
                        RECORDING_SEARCH_QUERY, e
                    );
                    return recordings;
                }
            };

            let json: serde_json::Value =
                match response.json().await {
                    Ok(json) => json,
                    Err(e) => {
                        eprintln!(
                            "[recordings] Search source query '{}' unparseable: {}",
                            RECORDING_SEARCH_QUERY, e
                        );
                        return recordings;
                    }
                };

            let hits = json["value"][0]["hitsContainers"][0]["hits"]
                .as_array()
                .cloned()
                .unwrap_or_default();

            let hit_count = hits.len();
            for hit in &hits {
                let resource = &hit["resource"];
                let Some(name) = resource["name"].as_str() else {
                    continue;
                };
                if !is_recording_video_name(name) {
                    continue;
                }

                let id = resource["id"].as_str().unwrap_or_default().to_string();
                if id.is_empty() {
                    continue;
                }
                let drive_id = resource["parentReference"]["driveId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                if drive_id.is_empty() {
                    continue;
                }
                // Files on the user's own drive are not "shared with me".
                if drive_id == own_drive_id {
                    continue;
                }

                recordings.push(MeetingRecording {
                    drive_id,
                    item: DriveItem {
                        id,
                        name: name.to_string(),
                        size: resource["size"].as_u64(),
                        last_modified: resource["lastModifiedDateTime"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        is_folder: false,
                        mime_type: resource["file"]["mimeType"]
                            .as_str()
                            .map(|s| s.to_string()),
                        web_url: resource["webUrl"].as_str().map(|s| s.to_string()),
                        parent_reference: None,
                        download_url: None,
                        created_date_time: resource["createdDateTime"]
                            .as_str()
                            .map(|s| s.to_string()),
                    },
                    source_type: RecordingSource::Shared,
                    source_name: String::new(),
                });
            }

            if hit_count < SEARCH_PAGE_SIZE {
                break;
            }
            from += SEARCH_PAGE_SIZE;
        }
    }

    eprintln!(
        "[recordings] Search source yielded {} recording(s)",
        recordings.len()
    );
    recordings
}

// ─────────────────────────────────────────────────────────────────────────────
// Tauri Commands
// ─────────────────────────────────────────────────────────────────────────────

/// List children of a drive item with pagination support.
#[tauri::command]
pub async fn list_files(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<Vec<DriveItem>, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let base = client.base_url();
    let mut all_items: Vec<DriveItem> = Vec::new();
    let mut url = format!(
        "{}/drives/{}/items/{}/children?$top=200&$select={}",
        base, drive_id, item_id, DRIVE_ITEM_SELECT
    );

    loop {
        let current_url = url.clone();
        let response = client
            .request_with_retry(&token, |http, tkn| http.get(&current_url).bearer_auth(tkn))
            .await?;

        let collection: GraphCollection<RawDriveItem> =
            response.json().await.map_err(|e| AppError::GraphApi {
                message: format!("Failed to parse response: {}", e),
                status_code: 0,
            })?;

        all_items.extend(collection.value.into_iter().map(DriveItem::from));

        match collection.next_link {
            Some(next) => url = next,
            None => break,
        }
    }

    Ok(all_items)
}

/// Get drive metadata.
#[tauri::command]
pub async fn get_drive(
    cloud_env: String,
    drive_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<Drive, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let url = format!("{}/drives/{}", client.base_url(), drive_id);

    let response = client
        .request_with_retry(&token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await?;

    let raw: RawDrive = response.json().await.map_err(|e| AppError::GraphApi {
        message: format!("Failed to parse response: {}", e),
        status_code: 0,
    })?;

    Ok(Drive::from(raw))
}

/// Get drive quota information.
#[tauri::command]
pub async fn get_drive_quota(
    cloud_env: String,
    drive_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<DriveQuota, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let url = format!("{}/drives/{}", client.base_url(), drive_id);

    let response = client
        .request_with_retry(&token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await?;

    let raw: RawDrive = response.json().await.map_err(|e| AppError::GraphApi {
        message: format!("Failed to parse response: {}", e),
        status_code: 0,
    })?;

    raw.quota
        .map(|q| DriveQuota {
            total: q.total.unwrap_or(0),
            used: q.used.unwrap_or(0),
            remaining: q.remaining.unwrap_or(0),
        })
        .ok_or_else(|| AppError::GraphApi {
            message: "Drive has no quota information".to_string(),
            status_code: 0,
        })
}

/// Search files within a drive. Scope can be "global" (from root) or "local" (from item_id).
#[tauri::command]
pub async fn search_files(
    cloud_env: String,
    drive_id: String,
    query: String,
    scope: String,
    item_id: Option<String>,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<Vec<DriveItem>, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let base = client.base_url();

    let url = if scope == "local" {
        if let Some(ref id) = item_id {
            format!(
                "{}/drives/{}/items/{}/search(q='{}')?$select={}",
                base, drive_id, id, query, DRIVE_ITEM_SELECT
            )
        } else {
            format!(
                "{}/drives/{}/root/search(q='{}')?$select={}",
                base, drive_id, query, DRIVE_ITEM_SELECT
            )
        }
    } else {
        format!(
            "{}/drives/{}/root/search(q='{}')?$select={}",
            base, drive_id, query, DRIVE_ITEM_SELECT
        )
    };

    let response = client
        .request_with_retry(&token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await?;

    let collection: GraphCollection<RawDriveItem> =
        response.json().await.map_err(|e| AppError::GraphApi {
            message: format!("Failed to parse search response: {}", e),
            status_code: 0,
        })?;

    Ok(collection.value.into_iter().map(DriveItem::from).collect())
}

/// Rename a drive item.
#[tauri::command]
pub async fn rename_item(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    new_name: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<DriveItem, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let url = format!(
        "{}/drives/{}/items/{}",
        client.base_url(),
        drive_id,
        item_id
    );

    let body = serde_json::json!({ "name": new_name });

    let response = client
        .request_with_retry(&token, |http, tkn| {
            http.patch(&url).bearer_auth(tkn).json(&body)
        })
        .await?;

    let raw: RawDriveItem = response.json().await.map_err(|e| AppError::GraphApi {
        message: format!("Failed to parse rename response: {}", e),
        status_code: 0,
    })?;

    Ok(DriveItem::from(raw))
}

/// Delete a drive item.
#[tauri::command]
pub async fn delete_item(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<(), AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let url = format!(
        "{}/drives/{}/items/{}",
        client.base_url(),
        drive_id,
        item_id
    );

    client
        .request_with_retry(&token, |http, tkn| http.delete(&url).bearer_auth(tkn))
        .await?;

    Ok(())
}

/// Create a new folder under a parent item.
#[tauri::command]
pub async fn create_folder(
    cloud_env: String,
    drive_id: String,
    parent_id: String,
    name: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<DriveItem, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let url = format!(
        "{}/drives/{}/items/{}/children",
        client.base_url(),
        drive_id,
        parent_id
    );

    let body = serde_json::json!({
        "name": name,
        "folder": {},
        "@microsoft.graph.conflictBehavior": "rename"
    });

    let response = client
        .request_with_retry(&token, |http, tkn| {
            http.post(&url).bearer_auth(tkn).json(&body)
        })
        .await?;

    let raw: RawDriveItem = response.json().await.map_err(|e| AppError::GraphApi {
        message: format!("Failed to parse create folder response: {}", e),
        status_code: 0,
    })?;

    Ok(DriveItem::from(raw))
}

/// Create a sharing link for a drive item.
#[tauri::command]
pub async fn create_share_link(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    options: ShareOptions,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<String, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let url = format!(
        "{}/drives/{}/items/{}/createLink",
        client.base_url(),
        drive_id,
        item_id
    );

    let mut body = serde_json::json!({
        "type": options.link_type,
        "scope": "anonymous"
    });

    if let Some(ref expiration) = options.expiration {
        body["expirationDateTime"] = serde_json::Value::String(expiration.clone());
    }
    if let Some(ref password) = options.password {
        body["password"] = serde_json::Value::String(password.clone());
    }

    let response = client
        .request_with_retry(&token, |http, tkn| {
            http.post(&url).bearer_auth(tkn).json(&body)
        })
        .await?;

    let share_response: ShareLinkResponse =
        response.json().await.map_err(|e| AppError::GraphApi {
            message: format!("Failed to parse share link response: {}", e),
            status_code: 0,
        })?;

    share_response
        .link
        .and_then(|l| l.web_url)
        .ok_or_else(|| AppError::GraphApi {
            message: "Share link response did not contain a URL".to_string(),
            status_code: 0,
        })
}

/// Convert a file to a different format and save to a local path.
#[tauri::command]
pub async fn convert_format(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    format: String,
    save_path: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<(), AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let url = format!(
        "{}/drives/{}/items/{}/content?format={}",
        client.base_url(),
        drive_id,
        item_id,
        format
    );

    let response = client
        .request_with_retry(&token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await?;

    let bytes = response.bytes().await.map_err(|e| AppError::Network {
        message: format!("Failed to download converted file: {}", e),
        retryable: false,
    })?;

    tokio::fs::write(&save_path, &bytes)
        .await
        .map_err(|e| AppError::FileSystem {
            message: format!("Failed to save converted file: {}", e),
            path: save_path,
        })?;

    Ok(())
}

/// Get a preview URL for a drive item.
#[tauri::command]
pub async fn get_preview_url(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<String, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let url = format!(
        "{}/drives/{}/items/{}/preview",
        client.base_url(),
        drive_id,
        item_id
    );

    let response = client
        .request_with_retry(&token, |http, tkn| {
            http.post(&url)
                .bearer_auth(tkn)
                .json(&serde_json::json!({}))
        })
        .await?;

    let preview: PreviewResponse = response.json().await.map_err(|e| AppError::GraphApi {
        message: format!("Failed to parse preview response: {}", e),
        status_code: 0,
    })?;

    preview.get_url.ok_or_else(|| AppError::GraphApi {
        message: "Preview response did not contain a URL".to_string(),
        status_code: 0,
    })
}

/// Get the best available thumbnail URL for an image or video drive item.
#[tauri::command]
pub async fn get_thumbnail_url(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<String, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let url = format!(
        "{}/drives/{}/items/{}/thumbnails",
        client.base_url(),
        drive_id,
        item_id
    );

    let response = client
        .request_with_retry(&token, |http, tkn| {
            http.get(&url).bearer_auth(tkn)
        })
        .await?;

    let collection: ThumbnailSetResponse =
        response.json().await.map_err(|e| AppError::GraphApi {
            message: format!("Failed to parse thumbnail response: {}", e),
            status_code: 0,
        })?;

    let set = collection.value.into_iter().next().ok_or_else(|| {
        AppError::GraphApi {
            message: "Item does not have a thumbnail set".to_string(),
            status_code: 0,
        }
    })?;

    set.large
        .or(set.medium)
        .or(set.small)
        .and_then(|thumbnail| thumbnail.url)
        .ok_or_else(|| AppError::GraphApi {
            message: "Thumbnail response did not contain a URL".to_string(),
            status_code: 0,
        })
}

/// Get the total size of a drive item. Folders are summed recursively.
#[tauri::command]
pub async fn get_item_size(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<u64, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let item_url = format!(
        "{}/drives/{}/items/{}?$select=id,size,folder,remoteItem,package",
        client.base_url(),
        drive_id,
        item_id
    );

    let response = client
        .request_with_retry(&token, |http, tkn| http.get(&item_url).bearer_auth(tkn))
        .await?;

    let raw: RawDriveItem = response.json().await.map_err(|e| AppError::GraphApi {
        message: format!("Failed to parse item size response: {}", e),
        status_code: 0,
    })?;
    let item = DriveItem::from(raw);

    if !item.is_folder {
        return Ok(item.size.unwrap_or(0));
    }

    sum_folder_size(&client, &token, &drive_id, &item_id, 0).await
}

/// Get properties of a specific drive item.
#[tauri::command]
pub async fn get_item_properties(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<DriveItem, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let url = format!(
        "{}/drives/{}/items/{}",
        client.base_url(),
        drive_id,
        item_id
    );

    let response = client
        .request_with_retry(&token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await?;

    let raw: RawDriveItem = response.json().await.map_err(|e| AppError::GraphApi {
        message: format!("Failed to parse item properties response: {}", e),
        status_code: 0,
    })?;

    Ok(DriveItem::from(raw))
}

/// Read a text-based file's content for Markdown/code/plain-text preview.
#[tauri::command]
pub async fn get_text_content(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    home_account_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<String, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_account(env.clone(), &home_account_id)
            .await?
    };

    let client = GraphClient::new(env);
    let bytes = download_item_bytes(&client, &token, &drive_id, &item_id).await?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(AppError::Validation {
            message: "File is too large for text preview".to_string(),
            field: "item_id".to_string(),
        });
    }

    Ok(String::from_utf8_lossy(&bytes).to_string())
}

/// Download a drive item's raw content bytes.
async fn download_item_bytes(
    client: &GraphClient,
    token: &str,
    drive_id: &str,
    item_id: &str,
) -> Result<Vec<u8>, AppError> {
    let url = format!(
        "{}/drives/{}/items/{}/content",
        client.base_url(),
        drive_id,
        item_id
    );

    let response = client
        .request_with_retry(token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await?;
    response.bytes().await.map_err(|e| AppError::Network {
        message: format!("Failed to read file content: {}", e),
        retryable: true,
    }).map(|b| b.to_vec())
}

/// Read a file's content and extract plain text for AI context injection.
/// Routes by extension: docx/pptx/xlsx/pdf are parsed into text; everything
/// else is treated as UTF-8 text.
#[tauri::command]
pub async fn extract_file_text(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    file_name: String,
    home_account_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<String, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_account(env.clone(), &home_account_id)
            .await?
    };

    let client = GraphClient::new(env);
    let bytes = download_item_bytes(&client, &token, &drive_id, &item_id).await?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(AppError::Validation {
            message: "File is too large for text extraction".to_string(),
            field: "item_id".to_string(),
        });
    }

    crate::content::extract_from_bytes(&file_name, &bytes)
}

/// Discover SharePoint sites visible to the account. Combines followed sites,
/// wildcard search, China M365 group discovery, and the tenant root fallback.
/// Failures of individual sources are ignored; duplicates are removed.
async fn discover_sites(
    client: &GraphClient,
    env: CloudEnvironment,
    token: &str,
) -> Vec<Site> {
    let base = client.base_url();
    let mut sites: Vec<Site> = Vec::new();

    // Followed sites are the most user-relevant source; search is the fallback.
    let discovery_urls = vec![
        format!("{}/me/followedSites?$select=id,displayName,webUrl", base),
        format!("{}/sites?search=*&$select=id,displayName,webUrl", base),
    ];
    for url in discovery_urls {
        if let Ok(found) = fetch_site_collection(client, token, &url).await {
            for site in found {
                push_unique_site(&mut sites, site);
            }
        }
    }

    // For China, also discover sites through unified M365 groups.
    if env == CloudEnvironment::China {
        let member_url = format!("{}/me/memberOf", base);
        if let Ok(response) = client
            .request_with_retry(token, |http, tkn| http.get(&member_url).bearer_auth(tkn))
            .await
        {
            if let Ok(members) = response.json::<GraphCollection<DirectoryObject>>().await {
                let group_ids: Vec<String> = members
                    .value
                    .into_iter()
                    .filter(|obj| {
                        obj.odata_type
                            .as_deref()
                            .map(|t| t == "#microsoft.graph.group")
                            .unwrap_or(false)
                    })
                    .filter_map(|obj| obj.id)
                    .collect();

                for group_id in group_ids {
                    let site_url = format!("{}/groups/{}/sites/root", base, group_id);
                    if let Ok(site_response) = client
                        .request_with_retry(token, |http, tkn| {
                            http.get(&site_url).bearer_auth(tkn)
                        })
                        .await
                    {
                        if let Ok(raw_site) = site_response.json::<RawSite>().await {
                            push_unique_site(&mut sites, Site::from(raw_site));
                        }
                    }
                }
            }
        }
    }

    // Last resort: the tenant root site, so the page still shows a real entry.
    if sites.is_empty() {
        let root_url = format!("{}/sites/root?$select=id,displayName,webUrl", base);
        if let Ok(response) = client
            .request_with_retry(token, |http, tkn| http.get(&root_url).bearer_auth(tkn))
            .await
        {
            if let Ok(raw_site) = response.json::<RawSite>().await {
                push_unique_site(&mut sites, Site::from(raw_site));
            }
        }
    }

    sites
}

/// Get SharePoint sites for the service picker page.
#[tauri::command]
pub async fn get_sharepoint_sites(
    cloud_env: String,
    home_account_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<Vec<Site>, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_account(env.clone(), &home_account_id)
            .await?
    };

    let client = GraphClient::new(env.clone());
    Ok(discover_sites(&client, env, &token).await)
}

/// Get drives for a specific SharePoint site.
#[tauri::command]
pub async fn get_site_drives(
    cloud_env: String,
    home_account_id: String,
    site_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<Vec<Drive>, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_account(env.clone(), &home_account_id)
            .await?
    };

    let client = GraphClient::new(env);
    let url = format!("{}/sites/{}/drives", client.base_url(), site_id);

    let response = client
        .request_with_retry(&token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await?;

    let collection: GraphCollection<RawDrive> =
        response.json().await.map_err(|e| AppError::GraphApi {
            message: format!("Failed to parse site drives response: {}", e),
            status_code: 0,
        })?;

    Ok(collection.value.into_iter().map(Drive::from).collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// Meeting Recordings
// ─────────────────────────────────────────────────────────────────────────────

/// Upper bounds that keep cross-site aggregation bounded on large tenants.
/// Cap for children pages fetched inside a single `Recordings` container.
const MAX_CHILDREN_PER_CONTAINER: usize = 500;

/// File extensions treated as Teams meeting recordings.
const RECORDING_VIDEO_EXTENSIONS: [&str; 1] = ["mp4"];

/// Folder-name aliases accepted when locating the OneDrive recordings folder,
/// covering tenants where the auto-created folder comes back localized.
/// Localized spellings of the OneDrive recordings folder. The user's default
/// UI language decides which one the auto-created folder gets, so probe them
/// all (matched case-insensitively).
const RECORDINGS_FOLDER_ALIASES: [&str; 10] = [
    "Recordings",       // en (and many untranslated tenants)
    "会议录制",          // zh-CN
    "录制",              // zh-CN alt
    "Grabaciones",      // es
    "Enregistrements",  // fr
    "Aufzeichnungen",   // de
    "Registrazioni",    // it
    "Gravações",        // pt
    "録画",              // ja
    "녹화",              // ko
];

/// Whether a file name looks like a meeting recording (video extension).
fn is_recording_video_name(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((_, ext)) => RECORDING_VIDEO_EXTENSIONS
            .iter()
            .any(|candidate| ext.eq_ignore_ascii_case(candidate)),
        None => false,
    }
}

/// Probe whether the signed-in user can actually download a drive item.
///
/// A withheld `@microsoft.graph.downloadUrl` is only one flavor of "download
/// blocked" — share-link block-download policies instead reject the `/content`
/// endpoint with 403 while the item metadata still lists a downloadUrl. So we
/// ask `/content` directly for a 1-byte range and let the status decide.
#[tauri::command]
pub async fn probe_download_allowed(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<bool, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let url = format!(
        "{}/drives/{}/items/{}/content",
        client.base_url(),
        drive_id,
        item_id
    );

    // 403/401 on /content IS the "download blocked" signal — request_with_retry
    // surfaces it as Err, so map it to `false` instead of propagating.
    match client
        .request_with_retry(&token, |http, tkn| {
            http.get(&url)
                .bearer_auth(tkn)
                .header("Range", "bytes=0-1")
        })
        .await
    {
        Ok(response) => Ok(response.status().is_success()),
        Err(AppError::GraphApi { status_code: 403, .. })
        | Err(AppError::GraphApi { status_code: 401, .. }) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Epoch seconds used to order recordings; unparseable dates sink to the bottom.
fn recording_epoch(recording: &MeetingRecording) -> i64 {
    DateTime::parse_from_rfc3339(&recording.item.last_modified)
        .ok()
        .map(|dt| dt.with_timezone(&Utc).timestamp())
        .unwrap_or(i64::MIN)
}

/// Sort recordings newest first (lastModifiedDateTime), name ascending as tiebreak.
fn sort_recordings_desc(recordings: &mut [MeetingRecording]) {
    recordings.sort_by(|a, b| {
        recording_epoch(b)
            .cmp(&recording_epoch(a))
            .then_with(|| a.item.name.to_lowercase().cmp(&b.item.name.to_lowercase()))
    });
}

/// List all file children behind a Graph children/search URL with pagination.
/// When `include_folders` is set, folder items are returned as well.
async fn fetch_all_children(
    client: &GraphClient,
    token: &str,
    start_url: String,
    max_items: usize,
    include_folders: bool,
) -> Result<Vec<DriveItem>, AppError> {
    let mut items: Vec<DriveItem> = Vec::new();
    let mut url = start_url;

    loop {
        let current_url = url.clone();
        let response = client
            .request_with_retry(token, |http, tkn| http.get(&current_url).bearer_auth(tkn))
            .await?;

        let collection: GraphCollection<RawDriveItem> =
            response.json().await.map_err(|e| AppError::GraphApi {
                message: format!("Failed to parse response: {}", e),
                status_code: 0,
            })?;

        for raw in collection.value {
            let item = DriveItem::from(raw);
            if !item.id.is_empty() && (include_folders || !item.is_folder) {
                items.push(item);
                if items.len() >= max_items {
                    return Ok(items);
                }
            }
        }

        match collection.next_link {
            Some(next) => url = next,
            None => break,
        }
    }

    Ok(items)
}

/// Locate the OneDrive recordings folder by probing common localized names in
/// the drive root; returns the children of the first match.
async fn locate_localized_recordings_children(
    client: &GraphClient,
    token: &str,
    base: &str,
) -> Result<Vec<DriveItem>, AppError> {
    let root_url = format!(
        "{}/me/drive/root/children?$top=200&$select={}",
        base, DRIVE_ITEM_SELECT
    );
    let root_entries =
        fetch_all_children(client, token, root_url, 200, true).await?;

    let folder = root_entries.into_iter().find(|item| {
        item.is_folder
            && RECORDINGS_FOLDER_ALIASES
                .iter()
                .any(|alias| item.name.eq_ignore_ascii_case(alias))
    });

    let Some(folder) = folder else {
        return Err(AppError::GraphApi {
            message: "No recordings folder found in OneDrive root".to_string(),
            status_code: 404,
        });
    };

    let children_url = format!(
        "{}/me/drive/items/{}/children?$top=200&$select={}",
        base, folder.id, DRIVE_ITEM_SELECT
    );
    fetch_all_children(client, token, children_url, MAX_CHILDREN_PER_CONTAINER, false).await
}

fn children_into_recordings(
    items: Vec<DriveItem>,
    drive_id: &str,
    source_type: RecordingSource,
    source_name: &str,
) -> Vec<MeetingRecording> {
    items
        .into_iter()
        .filter(|item| is_recording_video_name(&item.name))
        .map(|item| MeetingRecording {
            drive_id: drive_id.to_string(),
            item,
            source_type,
            source_name: source_name.to_string(),
        })
        .collect()
}

/// Collect recordings from the signed-in user's OneDrive `Recordings` folder.
/// Returns the user's own drive id alongside the recordings so the search
/// source can exclude own-drive hits. Missing folder or permission problems
/// simply yield no recordings.
async fn collect_onedrive_recordings(
    client: &GraphClient,
    token: &str,
) -> (String, Vec<MeetingRecording>) {
    let base = client.base_url();

    // Resolve the user's own drive id so later thumbnails/downloads can address it.
    let drive_url = format!("{}/me/drive?$select=id", base);
    let drive_id = match client
        .request_with_retry(token, |http, tkn| http.get(&drive_url).bearer_auth(tkn))
        .await
    {
        Ok(response) => match response.json::<RawDrive>().await {
            Ok(drive) => drive.id.unwrap_or_default(),
            Err(e) => {
                eprintln!("[recordings] OneDrive source skipped: bad drive response: {}", e);
                return (String::new(), Vec::new());
            }
        },
        Err(e) => {
            eprintln!("[recordings] OneDrive source skipped: drive lookup failed: {}", e);
            return (String::new(), Vec::new());
        }
    };
    if drive_id.is_empty() {
        eprintln!("[recordings] OneDrive source skipped: empty drive id");
        return (String::new(), Vec::new());
    }

    let url = format!(
        "{}/me/drive/root:/Recordings/children?$top=200&$select={}",
        base, DRIVE_ITEM_SELECT
    );
    // Exact path first; on 404 probe localized aliases in the drive root so
    // tenants with a non-English recordings folder still resolve.
    let children = match fetch_all_children(
        client,
        token,
        url,
        MAX_CHILDREN_PER_CONTAINER,
        false,
    )
    .await
    {
        Ok(children) => children,
        Err(AppError::GraphApi { status_code: 404, .. }) => {
            match locate_localized_recordings_children(client, token, base).await {
                Ok(children) => children,
                Err(e) => {
                    eprintln!(
                        "[recordings] OneDrive recordings folder not found (localized probe failed): {}",
                        e
                    );
                    return (drive_id, Vec::new());
                }
            }
        }
        Err(e) => {
            eprintln!(
                "[recordings] OneDrive Recordings folder listing failed: {}",
                e
            );
            return (drive_id, Vec::new());
        }
    };
    (
        drive_id.clone(),
        children_into_recordings(children, &drive_id, RecordingSource::Own, ""),
    )
}

/// Aggregate Teams meeting recordings visible to the account:
/// organizer OneDrive recordings plus channel-meeting recordings on SharePoint.
///
/// 21Vianet is not supported for this feature yet: SharePoint recordings there
/// rely on groups discovery and Graph communications APIs are unavailable.
#[tauri::command]
pub async fn get_meeting_recordings(
    cloud_env: String,
    home_account_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<Vec<MeetingRecording>, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    if env == CloudEnvironment::China {
        return Ok(Vec::new());
    }
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_account(env.clone(), &home_account_id)
            .await?
    };

    let client = Arc::new(GraphClient::new(env.clone()));

    let (own_drive_id, mut recordings) = collect_onedrive_recordings(&client, &token).await;

    // The "shared with me" half: .mp4 files other people shared with the user,
    // via Microsoft Search (/search/query) because /me/drive/sharedWithMe is
    // deprecated and returns far fewer items than the web UI shows. Hits on
    // the user's own drive are excluded there, so the two sources don't overlap.
    if env == CloudEnvironment::Global {
        recordings.extend(collect_search_recordings(&client, &token, &own_drive_id).await);
    }

    // Register discovered drive ids so later per-drive commands (thumbnails,
    // previews) can mint tokens the same way OneDrive tabs already do.
    {
        let mut auth = auth_module.lock().await;
        for recording in &recordings {
            auth.register_drive_mapping(&env, &recording.drive_id, &home_account_id);
        }
    }

    // Dedupe across sources, then show newest first.
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    recordings
        .retain(|recording| seen.insert((recording.drive_id.clone(), recording.item.id.clone())));
    sort_recordings_desc(&mut recordings);

    Ok(recordings)
}

/// Strip the final file extension ("a.b.mp4" -> "a.b"); names without a dot
/// are returned unchanged.
fn file_base_name(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((base, _)) => base,
        None => name,
    }
}

/// Does `child_name` name the transcript of a recording whose base name is
/// `recording_base_lower` (already lowercased)? Teams stores the transcript
/// as `{recording}.vtt` next to the recording; rank 0 is that exact match,
/// rank 1 covers variants that extend the base name (language suffixes).
fn transcript_match_rank(child_name: &str, recording_base_lower: &str) -> Option<u8> {
    let (child_base, ext) = child_name.rsplit_once('.')?;
    if !ext.eq_ignore_ascii_case("vtt") {
        return None;
    }
    let child_base_lower = child_base.to_lowercase();
    if child_base_lower == recording_base_lower {
        Some(0)
    } else if !recording_base_lower.is_empty() && child_base_lower.starts_with(recording_base_lower)
    {
        Some(1)
    } else {
        None
    }
}

/// Normalize a VTT cue timestamp ("00:05:12.340" / "05:12.340") to "[00:05:12]".
fn format_cue_timestamp(raw: &str) -> String {
    let main = raw.trim().split(['.', ',']).next().unwrap_or("");
    let mut parts: Vec<&str> = main.split(':').filter(|p| !p.is_empty()).collect();
    while parts.len() < 3 {
        parts.insert(0, "0");
    }
    let len = parts.len();
    let padded: Vec<String> = parts[len - 3..]
        .iter()
        .map(|p| format!("{:0>2}", p))
        .collect();
    format!("[{}]", padded.join(":"))
}

/// Remove VTT inline markup (voice spans `<v Name>`, color/class spans, word
/// timing tags) from a cue text line.
fn strip_vtt_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut depth = 0usize;
    for ch in line.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Unwrap bracketed speaker forms like `["John Doe"]` / `[John Doe]`.
fn unbracket_speaker(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() >= 2 {
        trimmed[1..trimmed.len() - 1]
            .trim_matches('"')
            .trim()
            .to_string()
    } else {
        trimmed.to_string()
    }
}

/// Split a cue's text lines into (speaker, text). Teams puts the speaker on
/// its own line before the utterance; single-line cues may carry a
/// `Speaker: text` prefix instead. Lines without any speaker hint yield None.
fn split_cue_speaker(lines: &[String]) -> (Option<String>, String) {
    if lines.is_empty() {
        return (None, String::new());
    }
    if lines.len() >= 2 {
        let speaker = unbracket_speaker(&lines[0]);
        let text = lines[1..]
            .iter()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        return (Some(speaker), text);
    }
    // Single line: `Speaker: text` only when the colon sits in a plausible
    // name prefix (≤4 words, colon followed by a space), so URLs and clock
    // times inside the text are left alone.
    let line = strip_vtt_tags(&lines[0]);
    let trimmed = line.trim();
    if let Some((prefix, rest)) = trimmed.split_once(": ") {
        let prefix = prefix.trim();
        let rest = rest.trim();
        let word_count = if prefix.is_empty() { 0 } else { prefix.split(' ').count() };
        if !rest.is_empty() && word_count >= 1 && word_count <= 4 {
            return (Some(unbracket_speaker(prefix)), rest.to_string());
        }
    }
    (None, trimmed.to_string())
}

/// One flattened VTT cue: normalized start timestamp, optional speaker, text.
type Cue = (String, Option<String>, String);

/// Finalize the cue being accumulated, if any.
fn push_cue(cues: &mut Vec<Cue>, start: &mut Option<String>, lines: &mut Vec<String>) {
    if let Some(start_ts) = start.take() {
        let (speaker, text) = split_cue_speaker(lines);
        cues.push((start_ts, speaker, text));
    }
    lines.clear();
}

/// Convert a Teams meeting transcript VTT into plain-text script lines:
/// `[hh:mm:ss] Speaker: text`, merging consecutive cues by the same speaker
/// into one line (the "grouped" export shape). Metadata (`WEBVTT`, `Kind:`,
/// `Language:`) and `NOTE`/`STYLE` blocks are dropped.
fn vtt_to_script_lines(vtt: &str) -> Vec<String> {
    let mut cues: Vec<Cue> = Vec::new();
    let mut cue_start: Option<String> = None;
    let mut cue_lines: Vec<String> = Vec::new();

    for raw_line in vtt.lines() {
        let line = raw_line.trim_end();
        if line.trim().is_empty() {
            push_cue(&mut cues, &mut cue_start, &mut cue_lines);
            continue;
        }
        if let Some(idx) = line.find("-->") {
            cue_start = Some(format_cue_timestamp(line[..idx].trim()));
            continue;
        }
        if cue_start.is_none() {
            // Header/metadata region (WEBVTT, Kind:, Language:) or a NOTE block.
            continue;
        }
        let cleaned = strip_vtt_tags(line);
        if !cleaned.trim().is_empty() {
            cue_lines.push(cleaned);
        }
    }
    push_cue(&mut cues, &mut cue_start, &mut cue_lines);

    render_script_lines(cues)
}

/// Flatten cues into script lines, merging consecutive cues by the same
/// speaker into one line.
fn render_script_lines(cues: Vec<Cue>) -> Vec<String> {
    let mut lines_out: Vec<String> = Vec::new();
    let mut last_speaker: Option<String> = None;
    for (start, speaker, text) in cues {
        if text.is_empty() {
            continue;
        }
        // Same speaker keeps talking: append to the line already on the page.
        if speaker.is_some() && speaker == last_speaker {
            if let Some(prev) = lines_out.last_mut() {
                prev.push(' ');
                prev.push_str(&text);
                continue;
            }
        }
        // Only cues with a timing line reach here, so start is always set.
        let line = match &speaker {
            Some(s) => format!("{} {}: {}", start, s, text),
            None => format!("{} {}", start, text),
        };
        lines_out.push(line);
        last_speaker = speaker;
    }
    lines_out
}

/// One entry of the Teams transcript JSON served by
/// `temporaryDownloadUrl&format=json` (see ms-teams-sharepoint-downloader).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTranscriptEntry {
    #[serde(default)]
    start_offset: Option<String>,
    #[serde(default)]
    speaker_display_name: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTranscriptJson {
    #[serde(default)]
    entries: Vec<RawTranscriptEntry>,
}

/// Parse a transcript time offset ("HH:MM:SS[.fff]" or raw seconds) into
/// "[hh:mm:ss]".
fn parse_time_offset(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.contains(':') {
        return Some(format_cue_timestamp(raw));
    }
    let secs: f64 = raw.parse().ok()?;
    if !secs.is_finite() || secs < 0.0 {
        return None;
    }
    let total = secs as u64;
    Some(format!(
        "[{:02}:{:02}:{:02}]",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    ))
}

/// Convert Teams transcript JSON (`{ entries: [...] }`) into cues.
fn json_transcript_to_cues(json: &str) -> Option<Vec<Cue>> {
    let parsed: RawTranscriptJson = serde_json::from_str(json).ok()?;
    Some(
        parsed
            .entries
            .into_iter()
            .filter_map(|entry| {
                let text = entry.text?.trim().to_string();
                if text.is_empty() {
                    return None;
                }
                let start = entry
                    .start_offset
                    .as_deref()
                    .and_then(parse_time_offset)
                    .unwrap_or_default();
                let speaker = entry.speaker_display_name.filter(|s| !s.trim().is_empty());
                Some((start, speaker, text))
            })
            .collect(),
    )
}

/// Convert a captured transcript payload — VTT or Teams transcript JSON —
/// into plain-text script lines.
fn transcript_to_script_lines(text: &str) -> Vec<String> {
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with('{') {
        if let Some(cues) = json_transcript_to_cues(trimmed) {
            let lines = render_script_lines(cues);
            if !lines.is_empty() {
                return lines;
            }
        }
    }
    vtt_to_script_lines(text)
}

/// Characters kept unescaped inside a Graph item path segment.
const ITEM_PATH_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Percent-encode each segment of a Graph item path (names may contain
/// spaces, `#`, `&`...). "/Recordings/My meeting" -> "/Recordings/My%20meeting".
fn encode_item_path(path: &str) -> String {
    path.split('/')
        .map(|segment| utf8_percent_encode(segment, ITEM_PATH_SEGMENT).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Build the drive-relative address (`root:/…`) for a file inside the folder
/// described by a Graph `parentReference.path` ("/drive/root:/Recordings" for
/// OneDrive, "/drives/{id}/root:/General/Recordings" for SharePoint).
fn item_path_under_parent(parent_path: &str, file_name: &str) -> String {
    let root_relative = parent_path
        .split_once("root:")
        .map(|(_, rest)| rest.trim_end_matches('/'))
        .unwrap_or("");
    format!("root:{}/{}", root_relative, file_name)
}

/// Fetch a drive item by addressing it under its parent folder path — no
/// folder enumeration, so this also works for shared recordings where the
/// user has file-level access but cannot list the parent folder.
async fn fetch_item_by_parent_path(
    client: &GraphClient,
    token: &str,
    drive_id: &str,
    parent_path: &str,
    file_name: &str,
) -> Result<DriveItem, AppError> {
    let url = format!(
        "{}/drives/{}/{}?$select={}",
        client.base_url(),
        drive_id,
        encode_item_path(&item_path_under_parent(parent_path, file_name)),
        DRIVE_ITEM_SELECT
    );
    let response = client
        .request_with_retry(token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await?;
    let raw: RawDriveItem = response.json().await.map_err(|e| AppError::GraphApi {
        message: format!("Failed to parse item response: {}", e),
        status_code: 0,
    })?;
    Ok(DriveItem::from(raw))
}

/// Locate the `.vtt` transcript Teams stores next to a meeting recording.
///
/// Primary strategy: address `{recording}.vtt` directly under the parent
/// folder path — shared recordings often grant file-level access only, so
/// folder listings may 404. Fallback: enumerate the parent folder to catch
/// language-variant transcript names; listing failures degrade to "no
/// candidates" rather than failing the export.
async fn find_recording_transcript(
    client: &GraphClient,
    token: &str,
    drive_id: &str,
    item_id: &str,
) -> Result<Option<DriveItem>, AppError> {
    let base = client.base_url();
    let item_url = format!(
        "{}/drives/{}/items/{}?$select={}",
        base, drive_id, item_id, DRIVE_ITEM_SELECT
    );
    let response = client
        .request_with_retry(token, |http, tkn| http.get(&item_url).bearer_auth(tkn))
        .await?;
    let raw: RawDriveItem = response.json().await.map_err(|e| AppError::GraphApi {
        message: format!("Failed to parse recording response: {}", e),
        status_code: 0,
    })?;
    let recording = DriveItem::from(raw);
    let Some(parent) = recording.parent_reference.as_ref() else {
        return Ok(None);
    };
    let parent_drive = if parent.drive_id.is_empty() {
        drive_id
    } else {
        parent.drive_id.as_str()
    };

    // 1) Exact `.vtt` twin, addressed by path.
    if let Some(parent_path) = parent.path.as_deref() {
        let twin = format!("{}.vtt", file_base_name(&recording.name));
        match fetch_item_by_parent_path(client, token, parent_drive, parent_path, &twin).await {
            Ok(item) => return Ok(Some(item)),
            Err(AppError::GraphApi { status_code: 404, .. })
            | Err(AppError::GraphApi { status_code: 403, .. }) => {}
            Err(e) => return Err(e),
        }
    }

    // 2) Fallback: enumerate the parent folder (language-variant names).
    let children_url = format!(
        "{}/drives/{}/items/{}/children?$top=200&$select={}",
        base, parent_drive, parent.id, DRIVE_ITEM_SELECT
    );
    let children = match fetch_all_children(
        client,
        token,
        children_url,
        MAX_CHILDREN_PER_CONTAINER,
        false,
    )
    .await
    {
        Ok(children) => children,
        Err(AppError::GraphApi { status_code: 404, .. })
        | Err(AppError::GraphApi { status_code: 403, .. }) => Vec::new(),
        Err(e) => return Err(e),
    };

    let recording_base = file_base_name(&recording.name).to_lowercase();
    let mut best: Option<(u8, DriveItem)> = None;
    for child in children {
        let Some(rank) = transcript_match_rank(&child.name, &recording_base) else {
            continue;
        };
        let better = best.as_ref().map_or(true, |(current, _)| rank < *current);
        if better {
            best = Some((rank, child));
        }
        if matches!(best.as_ref(), Some((0, _))) {
            break;
        }
    }
    Ok(best.map(|(_, item)| item))
}

/// Export a meeting recording's transcript as a plain-text script file.
///
/// Finds the `.vtt` transcript stored next to the recording, converts it to
/// `[hh:mm:ss] Speaker: text` lines and writes the text to `save_path`.
/// `Ok(None)` means the meeting has no transcript — the frontend surfaces a
/// dedicated "not transcribed" notice for that case.
#[tauri::command]
pub async fn export_recording_transcript(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    save_path: String,
    home_account_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<Option<TranscriptExport>, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_account(env.clone(), &home_account_id)
            .await?
    };

    let client = GraphClient::new(env);
    let Some(transcript) = find_recording_transcript(&client, &token, &drive_id, &item_id).await?
    else {
        return Ok(None);
    };

    let bytes = download_item_bytes(&client, &token, &drive_id, &transcript.id).await?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(AppError::Validation {
            message: "Transcript file is too large to export".to_string(),
            field: "item_id".to_string(),
        });
    }
    let vtt = String::from_utf8_lossy(&bytes);
    let lines = vtt_to_script_lines(&vtt);
    if lines.is_empty() {
        return Ok(None);
    }
    let mut content = lines.join("\n");
    content.push('\n');
    std::fs::write(&save_path, content).map_err(|e| AppError::FileSystem {
        message: format!("Failed to write transcript: {}", e),
        path: save_path.clone(),
    })?;

    Ok(Some(TranscriptExport {
        entry_count: lines.len() as u32,
        source_name: transcript.name.clone(),
    }))
}

/// Export a transcript payload already fetched by the in-page capture pipeline
/// (the embedded player's own transcript API, which works for shared
/// recordings the user cannot reach as drive files). Accepts VTT or the Teams
/// transcript JSON; converts to `[hh:mm:ss] Speaker: text` lines and writes
/// the text to `save_path`. `Ok(None)` when nothing usable remains.
#[tauri::command]
pub async fn export_transcript_text(
    save_path: String,
    transcript_text: String,
) -> Result<Option<TranscriptExport>, AppError> {
    if transcript_text.len() > 10 * 1024 * 1024 {
        return Err(AppError::Validation {
            message: "Transcript file is too large to export".to_string(),
            field: "save_path".to_string(),
        });
    }
    let lines = transcript_to_script_lines(&transcript_text);
    if lines.is_empty() {
        return Ok(None);
    }
    let mut content = lines.join("\n");
    content.push('\n');
    std::fs::write(&save_path, content).map_err(|e| AppError::FileSystem {
        message: format!("Failed to write transcript: {}", e),
        path: save_path.clone(),
    })?;

    Ok(Some(TranscriptExport {
        entry_count: lines.len() as u32,
        source_name: "player-capture".to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;




    fn recording(name: &str, modified: &str) -> MeetingRecording {
        MeetingRecording {
            drive_id: "drive".to_string(),
            item: DriveItem {
                id: name.to_string(),
                name: name.to_string(),
                size: Some(1),
                last_modified: modified.to_string(),
                is_folder: false,
                mime_type: Some("video/mp4".to_string()),
                web_url: None,
                parent_reference: None,
                download_url: None,
                created_date_time: None,
            },
            source_type: RecordingSource::Own,
            source_name: String::new(),
        }
    }

    #[test]
    fn video_extension_filter_accepts_mp4_only() {
        assert!(is_recording_video_name(
            "2026-08-20 14-30 - Sprint Review.MP4"
        ));

        // Teams recordings are .mp4; other video containers do not count.
        assert!(!is_recording_video_name("meeting.mkv"));
        assert!(!is_recording_video_name("clip.webm"));
        // Transcripts, documents and extension-less names are not recordings.
        assert!(!is_recording_video_name("meeting.vtt"));
        assert!(!is_recording_video_name("notes.txt"));
        assert!(!is_recording_video_name("noextension"));
        // ".mp4" hidden in the middle does not count as the extension.
        assert!(!is_recording_video_name("mp4.backup"));
    }

    #[test]
    fn recordings_path_matcher_accepts_localized_aliases_only() {
        assert!(path_points_to_recordings_folder(Some(&"/drive/root:/Recordings".to_string())));
        assert!(path_points_to_recordings_folder(Some(&"/drive/root:/recordings/sub".to_string())));
        assert!(path_points_to_recordings_folder(Some(&"/drive/root:/会议录制".to_string())));
        assert!(path_points_to_recordings_folder(Some(&"/drive/root:/录制/2026".to_string())));

        // Similar-looking segments are not folder-name matches.
        assert!(!path_points_to_recordings_folder(Some(
            &"/drive/root:/Documents/recordings-backup".to_string()
        )));
        assert!(!path_points_to_recordings_folder(None));
    }

    #[test]
    fn recordings_sort_newest_first_and_unparseable_dates_sink_to_bottom() {
        let mut recordings = vec![
            recording("old", "2026-08-19T10:00:00Z"),
            recording("bad", "not-a-date"),
            recording("newest", "2026-08-20T14:30:00Z"),
            recording("middle", "2026-08-20T06:30:00Z"),
        ];

        sort_recordings_desc(&mut recordings);

        let names: Vec<&str> = recordings.iter().map(|r| r.item.id.as_str()).collect();
        assert_eq!(names, vec!["newest", "middle", "old", "bad"]);
    }

    #[test]
    fn recordings_sort_tiebreaks_by_name_case_insensitively() {
        let mut recordings = vec![
            recording("Sprint", "2026-08-20T14:30:00Z"),
            recording("alpha", "2026-08-20T14:30:00Z"),
            recording("Beta", "2026-08-20T14:30:00Z"),
        ];

        sort_recordings_desc(&mut recordings);

        let names: Vec<&str> = recordings.iter().map(|r| r.item.id.as_str()).collect();
        assert_eq!(names, vec!["alpha", "Beta", "Sprint"]);
    }

    #[test]
    fn file_facet_maps_to_file() {
        let raw: RawDriveItem = serde_json::from_value(serde_json::json!({
            "id": "1",
            "name": "report.docx",
            "file": { "mimeType": "application/vnd.openxmlformats-officedocument.wordprocessingml.document" }
        }))
        .unwrap();

        let item = DriveItem::from(raw);
        assert!(!item.is_folder);
        assert_eq!(
            item.mime_type.as_deref(),
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
        );
    }

    #[test]
    fn folder_facet_maps_to_folder() {
        let raw: RawDriveItem = serde_json::from_value(serde_json::json!({
            "id": "2",
            "name": "Documents",
            "folder": {}
        }))
        .unwrap();

        assert!(DriveItem::from(raw).is_folder);
    }

    #[test]
    fn transcript_dispatch_handles_teams_json_entries() {
        // The JSON shape temporaryDownloadUrl&format=json returns; offsets
        // come both as raw seconds and HH:MM:SS strings.
        let json = concat!(
            "{\"entries\":[",
            "{\"startOffset\":\"5.2\",\"speakerDisplayName\":\"John Doe\",\"text\":\"Good morning.\"},",
            "{\"startOffset\":\"00:00:09\",\"speakerDisplayName\":\"John Doe\",\"text\":\"Let's start.\"},",
            "{\"startOffset\":\"62.5\",\"speakerDisplayName\":\"Jane Smith\",\"text\":\"Morning!\"}",
            "]}"
        );
        assert_eq!(
            transcript_to_script_lines(json),
            vec![
                "[00:00:05] John Doe: Good morning. Let's start.",
                "[00:01:02] Jane Smith: Morning!",
            ]
        );
        // VTT input still routes through the VTT parser.
        let vtt = "WEBVTT

00:00:03.000 --> 00:00:04.000
Hello.
";
        assert_eq!(transcript_to_script_lines(vtt), vec!["[00:00:03] Hello."]);
        // Unusable input yields nothing.
        assert!(transcript_to_script_lines("not a transcript").is_empty());
    }

    #[test]
    fn transcript_path_addresses_root_relative_folders_from_both_drive_flavors() {
        // OneDrive parentReference style.
        assert_eq!(
            item_path_under_parent("/drive/root:/Recordings", "Meeting.vtt"),
            "root:/Recordings/Meeting.vtt"
        );
        // Recording directly at the drive root.
        assert_eq!(
            item_path_under_parent("/drive/root:", "Meeting.vtt"),
            "root:/Meeting.vtt"
        );
        // SharePoint drives carry the drive id in the parent path.
        assert_eq!(
            item_path_under_parent("/drives/abc/root:/General/Recordings", "M.vtt"),
            "root:/General/Recordings/M.vtt"
        );
        // A trailing slash on the parent path must not double up.
        assert_eq!(
            item_path_under_parent("/drive/root:/Recordings/", "M.vtt"),
            "root:/Recordings/M.vtt"
        );
    }

    #[test]
    fn item_path_encoding_escapes_url_meaningful_characters() {
        assert_eq!(encode_item_path("/Recordings/My meeting"), "/Recordings/My%20meeting");
        assert_eq!(encode_item_path("/Recordings/Review #3"), "/Recordings/Review%20%233");
        // Reserved unreserved characters stay readable.
        assert_eq!(
            encode_item_path("/Recordings/Q3-review_final.v2"),
            "/Recordings/Q3-review_final.v2"
        );
    }

    #[test]
    fn transcript_match_prefers_exact_vtt_and_rejects_other_extensions() {
        let base = "2026-08-20 14-30 - sprint review".to_lowercase();

        // Exact {recording}.vtt twin is rank 0, any casing.
        assert_eq!(
            transcript_match_rank("2026-08-20 14-30 - Sprint Review.vtt", &base),
            Some(0)
        );
        assert_eq!(
            transcript_match_rank("2026-08-20 14-30 - SPRINT REVIEW.VTT", &base),
            Some(0)
        );
        // Extended names (language variants) still match, at lower rank.
        assert_eq!(
            transcript_match_rank("2026-08-20 14-30 - Sprint Review-en.vtt", &base),
            Some(1)
        );
        // Unrelated vtt files and non-vtt twins are not transcripts.
        assert_eq!(transcript_match_rank("other-meeting.vtt", &base), None);
        assert_eq!(
            transcript_match_rank("2026-08-20 14-30 - Sprint Review.docx", &base),
            None
        );
        assert_eq!(
            transcript_match_rank("2026-08-20 14-30 - Sprint Review.mp4", &base),
            None
        );
        // An empty recording base only matches a bare ".vtt" exactly.
        assert_eq!(transcript_match_rank("whatever.vtt", ""), None);
        assert_eq!(transcript_match_rank(".vtt", ""), Some(0));
    }

    #[test]
    fn file_base_name_strips_only_the_final_extension() {
        assert_eq!(file_base_name("meeting.mp4"), "meeting");
        assert_eq!(file_base_name("a.b.mp4"), "a.b");
        assert_eq!(file_base_name("noextension"), "noextension");
    }

    #[test]
    fn cue_timestamps_normalize_to_bracketed_hh_mm_ss() {
        assert_eq!(format_cue_timestamp("00:00:05.120"), "[00:00:05]");
        assert_eq!(format_cue_timestamp("01:02:03,456"), "[01:02:03]");
        // VTT permits minute-only timestamps; normalize to full h:m:s.
        assert_eq!(format_cue_timestamp("05:12.500"), "[00:05:12]");
    }

    #[test]
    fn vtt_script_converts_teams_speaker_lines_and_groups_consecutive_cues() {
        let vtt = "WEBVTT
Kind: captions
Language: en-US

            00:00:05.120 --> 00:00:08.400
John Doe
Good morning everyone.

            00:00:09.000 --> 00:00:11.000
John Doe
Let's get started.

            00:00:12.000 --> 00:00:13.500
Jane Smith
Morning!
";

        assert_eq!(
            vtt_to_script_lines(vtt),
            vec![
                "[00:00:05] John Doe: Good morning everyone. Let's get started.",
                "[00:00:12] Jane Smith: Morning!",
            ]
        );
    }

    #[test]
    fn vtt_script_handles_inline_speaker_and_speakerless_captions() {
        // Single-line `Speaker: text` cues.
        let inline = "WEBVTT

            00:00:03.250 --> 00:00:05.670
John Doe: Hello there.
";
        assert_eq!(
            vtt_to_script_lines(inline),
            vec!["[00:00:03] John Doe: Hello there."]
        );

        // Captions without any speaker attribution.
        let plain = "WEBVTT

            00:00:10.000 --> 00:00:12.000
Welcome to the review.
";
        assert_eq!(
            vtt_to_script_lines(plain),
            vec!["[00:00:10] Welcome to the review."]
        );

        // Bracketed speaker form, inline markup and NOTE blocks are cleaned.
        let decorated = "WEBVTT

NOTE this is a note
with two lines

            00:01:00.000 --> 00:01:02.000
[\"Jane\"]
<v Jane>Please <c.colorE5E7E5>look</c> here.</v>
";
        assert_eq!(
            vtt_to_script_lines(decorated),
            vec!["[00:01:00] Jane: Please look here."]
        );

        // Metadata-only input produces nothing usable.
        assert!(vtt_to_script_lines("WEBVTT
Kind: captions
Language: en-US
").is_empty());
    }

    #[test]
    fn remote_folder_and_package_map_to_folder() {
        let remote_folder: RawDriveItem = serde_json::from_value(serde_json::json!({
            "id": "3",
            "name": "Shared Folder",
            "remoteItem": { "folder": {} }
        }))
        .unwrap();
        assert!(DriveItem::from(remote_folder).is_folder);

        let package: RawDriveItem = serde_json::from_value(serde_json::json!({
            "id": "4",
            "name": "Notebook",
            "package": { "type": "oneNote" }
        }))
        .unwrap();
        assert!(DriveItem::from(package).is_folder);
    }
}
