use serde::Deserialize;
use tauri::State;
use tokio::sync::Mutex;

use crate::auth::cloud_config::CloudEnvironment;
use crate::auth::AuthModule;
use crate::errors::AppError;
use crate::graph::GraphClient;
use crate::models::{Drive, DriveItem, DriveQuota, ShareOptions, Site};

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
struct RawRemoteItem {
    folder: Option<serde_json::Value>,
    package: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RawFile {
    #[serde(rename = "mimeType")]
    mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawParentReference {
    #[serde(rename = "driveId")]
    drive_id: Option<String>,
    id: Option<String>,
    path: Option<String>,
    name: Option<String>,
}

impl From<RawDriveItem> for DriveItem {
    fn from(raw: RawDriveItem) -> Self {
        DriveItem {
            id: raw.id.unwrap_or_default(),
            name: raw.name.unwrap_or_default(),
            size: raw.size,
            last_modified: raw.last_modified_date_time.unwrap_or_default(),
            is_folder: raw.folder.is_some()
                || raw.package.is_some()
                || raw
                    .remote_item
                    .as_ref()
                    .and_then(|r| r.folder.as_ref().or(r.package.as_ref()))
                    .is_some(),
            mime_type: raw.file.and_then(|f| f.mime_type),
            web_url: raw.web_url,
            parent_reference: raw
                .parent_reference
                .map(|pr| crate::models::ParentReference {
                    drive_id: pr.drive_id.unwrap_or_default(),
                    id: pr.id.unwrap_or_default(),
                    path: pr.path,
                    name: pr.name,
                }),
            download_url: raw.download_url,
            created_date_time: raw.created_date_time,
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
    let url = format!(
        "{}/drives/{}/items/{}/content",
        client.base_url(),
        drive_id,
        item_id
    );

    let response = client
        .request_with_retry(&token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await?;
    let bytes = response.bytes().await.map_err(|e| AppError::Network {
        message: format!("Failed to read file content: {}", e),
        retryable: true,
    })?;

    if bytes.len() > 2 * 1024 * 1024 {
        return Err(AppError::Validation {
            message: "File is too large for text preview".to_string(),
            field: "item_id".to_string(),
        });
    }

    Ok(String::from_utf8_lossy(&bytes).to_string())
}

/// Get SharePoint sites. For Global uses /sites?search=*, for China uses memberOf group discovery.
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
    let base = client.base_url();
    let mut sites: Vec<Site> = Vec::new();

    // Followed sites are the most user-relevant source; search is the fallback.
    let discovery_urls = vec![
        format!("{}/me/followedSites?$select=id,displayName,webUrl", base),
        format!("{}/sites?search=*&$select=id,displayName,webUrl", base),
    ];
    for url in discovery_urls {
        if let Ok(found) = fetch_site_collection(&client, &token, &url).await {
            for site in found {
                push_unique_site(&mut sites, site);
            }
        }
    }

    // For China, also discover sites through unified M365 groups.
    if env == CloudEnvironment::China {
        let member_url = format!("{}/me/memberOf", base);
        if let Ok(response) = client
            .request_with_retry(&token, |http, tkn| http.get(&member_url).bearer_auth(tkn))
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
                        .request_with_retry(&token, |http, tkn| {
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
            .request_with_retry(&token, |http, tkn| http.get(&root_url).bearer_auth(tkn))
            .await
        {
            if let Ok(raw_site) = response.json::<RawSite>().await {
                push_unique_site(&mut sites, Site::from(raw_site));
            }
        }
    }

    Ok(sites)
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

/// Get drives shared with the current user.
#[tauri::command]
pub async fn get_shared_drives(
    cloud_env: String,
    home_account_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<Vec<Drive>, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_account(env.clone(), &home_account_id)
            .await?
    };

    let client = GraphClient::new(env);
    let url = format!("{}/me/drive/sharedWithMe", client.base_url());

    let response = client
        .request_with_retry(&token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await?;

    let collection: GraphCollection<RawDrive> =
        response.json().await.map_err(|e| AppError::GraphApi {
            message: format!("Failed to parse shared drives response: {}", e),
            status_code: 0,
        })?;

    Ok(collection.value.into_iter().map(Drive::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

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
