use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use tauri::State;
use tokio::sync::Mutex;

use crate::auth::cloud_config::CloudEnvironment;
use crate::auth::AuthModule;
use crate::errors::AppError;
use crate::graph::GraphClient;
use crate::models::{
    Drive, DriveItem, DriveQuota, MeetingRecording, RecordingSource, ShareOptions, Site,
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
