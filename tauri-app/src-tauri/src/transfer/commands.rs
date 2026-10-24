use std::path::PathBuf;

use futures::StreamExt;
use serde::Deserialize;
use tauri::State;
use tokio::sync::Mutex;

use crate::auth::cloud_config::CloudEnvironment;
use crate::auth::AuthModule;
use crate::errors::AppError;
use crate::graph::GraphClient;
use crate::transfer::download::{
    BatchFileRecord, BatchInfo, BatchSnapshot, DownloadEngine, DownloadParams, FileVerifyResult,
    verify_download_integrity,
};
use crate::transfer::stream_bridge::{start_upload_server, StreamDownloadChannel};
use crate::transfer::upload::{upload_folder_recursive, UploadEngine, UploadParams};

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

fn env_string(env: &CloudEnvironment) -> String {
    match env {
        CloudEnvironment::Global => "global".to_string(),
        CloudEnvironment::China => "china".to_string(),
    }
}

/// Graph API collection response wrapper.
#[derive(Debug, Deserialize)]
struct GraphCollection<T> {
    value: Vec<T>,
    #[serde(rename = "@odata.nextLink")]
    #[allow(dead_code)]
    next_link: Option<String>,
}

/// Raw drive item as returned by Graph API (for download URL extraction and folder listing).
#[derive(Debug, Deserialize)]
struct RawDriveItem {
    id: Option<String>,
    name: Option<String>,
    size: Option<u64>,
    folder: Option<serde_json::Value>,
    #[serde(rename = "remoteItem")]
    remote_item: Option<TransferRemoteItem>,
    package: Option<serde_json::Value>,
    file: Option<TransferFileMeta>,
}

#[derive(Debug, Deserialize)]
struct TransferFileMeta {
    hashes: Option<TransferHashes>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransferHashes {
    quick_xor_hash: Option<String>,
}

impl RawDriveItem {
    fn quick_xor_hash(&self) -> Option<String> {
        self.file
            .as_ref()
            .and_then(|f| f.hashes.as_ref())
            .and_then(|h| h.quick_xor_hash.clone())
            .or_else(|| {
                self.remote_item
                    .as_ref()
                    .and_then(|r| r.file.as_ref())
                    .and_then(|f| f.hashes.as_ref())
                    .and_then(|h| h.quick_xor_hash.clone())
            })
    }
}

/// Fetch an item's quickXorHash from Graph. Returns None when the item has no
/// hash or the lookup fails — verification then falls back to size-only.
async fn fetch_quick_xor_hash(
    client: &GraphClient,
    token: &str,
    drive_id: &str,
    item_id: &str,
) -> Option<String> {
    let url = format!(
        "{}/drives/{}/items/{}?$select=file",
        client.base_url(),
        drive_id,
        item_id
    );
    let response = match client
        .request_with_retry(token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await
    {
        Ok(response) => response,
        Err(_) => return None,
    };
    let json: serde_json::Value = response.json().await.ok()?;
    json["file"]["hashes"]["quickXorHash"]
        .as_str()
        .map(str::to_string)
}

/// A single file to include in a batch download.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDownloadSpec {
    item_id: String,
    file_name: String,
    file_size: u64,
}

#[derive(Debug, Deserialize)]
struct TransferRemoteItem {
    folder: Option<serde_json::Value>,
    package: Option<serde_json::Value>,
    file: Option<TransferFileMeta>,
}

/// Recursively list all files in a folder, creating local directories as needed.
/// Returns a list of (file_name, item_id, size, relative_path) tuples for all files.
async fn list_folder_recursive(
    client: &GraphClient,
    token: &str,
    drive_id: &str,
    folder_id: &str,
    local_base: &PathBuf,
) -> Result<Vec<(String, String, u64, PathBuf, Option<String>)>, AppError> {
    let mut files = Vec::new();
    let base = client.base_url();

    let mut url = format!(
        "{}/drives/{}/items/{}/children?$top=200",
        base, drive_id, folder_id
    );

    loop {
        let current_url = url.clone();
        let response = client
            .request_with_retry(token, |http, tkn| http.get(&current_url).bearer_auth(tkn))
            .await?;

        let collection: GraphCollection<RawDriveItem> =
            response.json().await.map_err(|e| AppError::GraphApi {
                message: format!("Failed to parse folder children: {}", e),
                status_code: 0,
            })?;

        for item in collection.value {
            let quick_xor_hash = item.quick_xor_hash();
            let name = item.name.unwrap_or_default();
            let id = item.id.unwrap_or_default();

            if item.folder.is_some()
                || item.package.is_some()
                || item
                    .remote_item
                    .as_ref()
                    .and_then(|r| r.folder.as_ref().or(r.package.as_ref()))
                    .is_some()
            {
                // Create local directory and recurse
                let subfolder_path = local_base.join(&name);
                tokio::fs::create_dir_all(&subfolder_path)
                    .await
                    .map_err(|e| AppError::FileSystem {
                        message: format!("Failed to create directory: {}", e),
                        path: subfolder_path.to_string_lossy().to_string(),
                    })?;

                let mut sub_files = Box::pin(list_folder_recursive(
                    client,
                    token,
                    drive_id,
                    &id,
                    &subfolder_path,
                ))
                .await?;
                files.append(&mut sub_files);
            } else {
                // File: add to the list
                let size = item.size.unwrap_or(0);
                let file_path = local_base.join(&name);
                files.push((name, id, size, file_path, quick_xor_hash));
            }
        }

        match collection.next_link {
            Some(next) => url = next,
            None => break,
        }
    }

    Ok(files)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tauri Commands
// ─────────────────────────────────────────────────────────────────────────────

/// Download a single file from OneDrive/SharePoint.
///
/// Gets the download URL from Graph API and enqueues a chunked download task.
/// Returns the task ID for tracking progress.
#[tauri::command]
pub async fn download_file(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    home_account_id: String,
    file_name: String,
    file_size: u64,
    local_path: String,
    auth_module: State<'_, Mutex<AuthModule>>,
    download_engine: State<'_, Mutex<DownloadEngine>>,
) -> Result<BatchInfo, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let env_value = env_string(&env);
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let download_url = format!(
        "{}/drives/{}/items/{}/content",
        client.base_url(),
        drive_id,
        item_id
    );
    let quick_xor_hash = fetch_quick_xor_hash(&client, &token, &drive_id, &item_id).await;

    // Create a single-file batch so the UI always sees one downloadable task.
    let mut engine = download_engine.lock().await;
    engine
        .create_batch(
            file_name.clone(),
            vec![DownloadParams {
                file_name,
                home_account_id,
                cloud_env: env_value,
                drive_id,
                item_id,
                local_path: PathBuf::from(local_path),
                total_bytes: file_size,
                download_url,
                bearer_token: Some(token.clone()),
                quick_xor_hash,
            }],
        )
        .await
}

/// Download multiple selected files into one local directory as a single batch task.
#[tauri::command]
pub async fn download_files(
    cloud_env: String,
    drive_id: String,
    home_account_id: String,
    files: Vec<FileDownloadSpec>,
    local_dir: String,
    batch_name: String,
    auth_module: State<'_, Mutex<AuthModule>>,
    download_engine: State<'_, Mutex<DownloadEngine>>,
) -> Result<BatchInfo, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env.clone());
    let local_base = PathBuf::from(&local_dir);
    tokio::fs::create_dir_all(&local_base)
        .await
        .map_err(|e| AppError::FileSystem {
            message: format!("Failed to create directory: {}", e),
            path: local_dir.clone(),
        })?;

    // Owned values only in the per-file future: borrowed closures inside a
    // buffered stream trip the tauri command macro's lifetime analysis.
    let item_ids: Vec<String> = files.iter().map(|f| f.item_id.clone()).collect();
    let token_for_hash = token.clone();
    let drive_for_hash = drive_id.clone();
    let hashes: Vec<Option<String>> = futures::stream::iter(item_ids)
        .map(|item_id| {
            let client = GraphClient::new(env.clone());
            let token = token_for_hash.clone();
            let drive_id = drive_for_hash.clone();
            async move { fetch_quick_xor_hash(&client, &token, &drive_id, &item_id).await }
        })
        .buffer_unordered(6)
        .collect()
        .await;

    let params = files
        .into_iter()
        .zip(hashes)
        .map(|(file, quick_xor_hash)| {
            let download_url = format!(
                "{}/drives/{}/items/{}/content",
                client.base_url(),
                drive_id,
                file.item_id
            );
            DownloadParams {
                file_name: file.file_name.clone(),
                home_account_id: home_account_id.clone(),
                cloud_env: cloud_env.clone(),
                drive_id: drive_id.clone(),
                item_id: file.item_id,
                local_path: local_base.join(&file.file_name),
                total_bytes: file.file_size,
                download_url,
                bearer_token: Some(token.clone()),
                quick_xor_hash,
            }
        })
        .collect::<Vec<_>>();

    let mut engine = download_engine.lock().await;
    engine.create_batch(batch_name, params).await
}

/// Download an entire folder recursively as a single batch task.
#[tauri::command]
pub async fn download_folder(
    cloud_env: String,
    drive_id: String,
    item_id: String,
    home_account_id: String,
    local_path: String,
    batch_name: String,
    auth_module: State<'_, Mutex<AuthModule>>,
    download_engine: State<'_, Mutex<DownloadEngine>>,
) -> Result<BatchInfo, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let local_base = PathBuf::from(&local_path);

    tokio::fs::create_dir_all(&local_base)
        .await
        .map_err(|e| AppError::FileSystem {
            message: format!("Failed to create directory: {}", e),
            path: local_path.clone(),
        })?;

    let files = list_folder_recursive(&client, &token, &drive_id, &item_id, &local_base).await?;

    let params = files
        .into_iter()
        .map(|(file_name, file_item_id, file_size, file_path, quick_xor_hash)| {
            let download_url = format!(
                "{}/drives/{}/items/{}/content",
                client.base_url(),
                drive_id,
                file_item_id
            );
            DownloadParams {
                file_name,
                home_account_id: home_account_id.clone(),
                cloud_env: cloud_env.clone(),
                drive_id: drive_id.clone(),
                item_id: file_item_id,
                local_path: file_path,
                total_bytes: file_size,
                download_url,
                bearer_token: Some(token.clone()),
                quick_xor_hash,
            }
        })
        .collect::<Vec<_>>();

    let mut engine = download_engine.lock().await;
    engine.create_batch(batch_name, params).await
}

/// Return all persisted download batches for startup restore.
#[tauri::command]
pub async fn get_download_tasks(
    download_engine: State<'_, Mutex<DownloadEngine>>,
) -> Result<Vec<BatchSnapshot>, AppError> {
    Ok(download_engine.lock().await.snapshot_batches())
}

/// Verify every downloaded file of a batch against the cloud copy: size
/// always, quickXorHash whenever the cloud provides one (persisted for new
/// tasks, fetched per-item from Graph for legacy ones).
#[tauri::command]
pub async fn verify_batch_files(
    cloud_env: String,
    task_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
    download_engine: State<'_, Mutex<DownloadEngine>>,
) -> Result<Vec<FileVerifyResult>, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let (home_account_id, records) = {
        let engine = download_engine.lock().await;
        engine
            .batch_files_for_verification(&task_id)
            .ok_or_else(|| AppError::Transfer {
                message: "Task not found".to_string(),
                task_id: task_id.clone(),
            })?
    };
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_account(env.clone(), &home_account_id)
            .await?
    };

    // Owned values only in the per-file future (see the note in download_files).
    let results: Vec<FileVerifyResult> = futures::stream::iter(records)
        .map(|record| {
            let client = GraphClient::new(env.clone());
            let token = token.clone();
            async move { verify_one_file(record, &client, &token).await }
        })
        .buffer_unordered(4)
        .collect()
        .await;
    Ok(results)
}

async fn verify_one_file(
    record: BatchFileRecord,
    client: &GraphClient,
    token: &str,
) -> FileVerifyResult {
    let file_name = record.file_name.clone();
    if record.status != "completed" {
        return FileVerifyResult {
            file_name,
            status: "not-downloaded".to_string(),
            detail: None,
        };
    }
    let path = PathBuf::from(&record.local_path);
    if !path.exists() {
        return FileVerifyResult {
            file_name,
            status: "missing".to_string(),
            detail: None,
        };
    }

    // Tasks that finished before hash verification existed have no persisted
    // hash: ask Graph for the item's quickXorHash so the check covers
    // content, not just size.
    let mut hash = record.quick_xor_hash.clone();
    if hash.is_none() {
        hash = fetch_quick_xor_hash(client, token, &record.drive_id, &record.item_id).await;
        if hash.is_none() {
            return match tokio::fs::metadata(&path).await {
                Ok(meta) if meta.len() == record.total_bytes => FileVerifyResult {
                    file_name,
                    status: "hash-unavailable".to_string(),
                    detail: Some("size ok; cloud hash unavailable".to_string()),
                },
                Ok(meta) => FileVerifyResult {
                    file_name,
                    status: "size-mismatch".to_string(),
                    detail: Some(format!(
                        "expected {} bytes, got {}",
                        record.total_bytes,
                        meta.len()
                    )),
                },
                Err(e) => FileVerifyResult {
                    file_name,
                    status: "missing".to_string(),
                    detail: Some(e.to_string()),
                },
            };
        }
    }

    match verify_download_integrity(&path, record.total_bytes, hash.as_deref()).await {
        Ok(()) => FileVerifyResult {
            file_name,
            status: "verified".to_string(),
            detail: None,
        },
        Err(e) => {
            let status = if e.contains("size mismatch") {
                "size-mismatch"
            } else {
                "hash-mismatch"
            };
            FileVerifyResult {
                file_name,
                status: status.to_string(),
                detail: Some(e),
            }
        }
    }
}

/// Re-download the files a verification flagged as missing or corrupt: they
/// are dropped from the old batch (bad local artifacts deleted) and queued
/// again under a fresh batch at the same local paths.
#[tauri::command]
pub async fn redownload_files(
    cloud_env: String,
    task_id: String,
    file_names: Vec<String>,
    new_batch_name: String,
    auth_module: State<'_, Mutex<AuthModule>>,
    download_engine: State<'_, Mutex<DownloadEngine>>,
) -> Result<BatchInfo, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let home_account_id = {
        let engine = download_engine.lock().await;
        engine.batch_home_account_id(&task_id).ok_or_else(|| AppError::Transfer {
            message: "Task not found".to_string(),
            task_id: task_id.clone(),
        })?
    };
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_account(env.clone(), &home_account_id)
            .await?
    };
    let client = GraphClient::new(env.clone());
    let base_url = client.base_url().to_string();

    let mut params = {
        let mut engine = download_engine.lock().await;
        engine
            .take_completed_files_for_redownload(
                &task_id,
                &file_names,
                &base_url,
                &token,
                env_string(&env).as_str(),
            )
            .await
    };
    if params.is_empty() {
        return Err(AppError::Transfer {
            message: "No files matched for re-download".to_string(),
            task_id,
        });
    }

    // Fetch hashes for files whose legacy records lack one, so the fresh
    // tasks verify on completion like any new download.
    let missing: Vec<String> = params
        .iter()
        .filter(|p| p.quick_xor_hash.is_none())
        .map(|p| p.item_id.clone())
        .collect();
    if !missing.is_empty() {
        let keys = missing.clone();
        let fetched: Vec<Option<String>> = futures::stream::iter(missing)
            .map(|item_id| {
                let client = GraphClient::new(env.clone());
                let token = token.clone();
                let drive_id = params
                    .iter()
                    .find(|p| p.item_id == item_id)
                    .map(|p| p.drive_id.clone())
                    .unwrap_or_default();
                async move { fetch_quick_xor_hash(&client, &token, &drive_id, &item_id).await }
            })
            .buffer_unordered(6)
            .collect()
            .await;
        let by_item: std::collections::HashMap<String, Option<String>> = keys
            .into_iter()
            .zip(fetched)
            .collect();
        for param in params.iter_mut() {
            if param.quick_xor_hash.is_none() {
                param.quick_xor_hash = by_item.get(&param.item_id).cloned().flatten();
            }
        }
    }

    let mut engine = download_engine.lock().await;
    engine.create_batch(new_batch_name, params).await
}

/// Pause a batch download task.
#[tauri::command]
pub async fn pause_download(
    task_id: String,
    download_engine: State<'_, Mutex<DownloadEngine>>,
) -> Result<(), AppError> {
    let mut engine = download_engine.lock().await;
    engine.pause_batch(&task_id).await
}

/// Resume a paused batch download task with a fresh Graph URL.
#[tauri::command]
pub async fn resume_download(
    cloud_env: String,
    task_id: String,
    auth_module: State<'_, Mutex<AuthModule>>,
    download_engine: State<'_, Mutex<DownloadEngine>>,
) -> Result<(), AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let home_account_id = {
        let engine = download_engine.lock().await;
        engine.batch_home_account_id(&task_id).ok_or_else(|| {
            AppError::Transfer {
                message: "Task not found".to_string(),
                task_id: task_id.clone(),
            }
        })?
    };
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_account(env.clone(), &home_account_id)
            .await?
    };
    let client = GraphClient::new(env);
    let base_url = client.base_url().to_string();

    let mut engine = download_engine.lock().await;
    engine.resume_batch(&task_id, &base_url, token).await
}

/// Cancel a batch download task and remove partial files.
#[tauri::command]
pub async fn cancel_download(
    task_id: String,
    download_engine: State<'_, Mutex<DownloadEngine>>,
) -> Result<(), AppError> {
    let mut engine = download_engine.lock().await;
    engine.cancel_batch(&task_id)
}

/// Remove a download batch from the task list. Completed files are kept.
#[tauri::command]
pub async fn remove_download(
    task_id: String,
    download_engine: State<'_, Mutex<DownloadEngine>>,
) -> Result<(), AppError> {
    let mut engine = download_engine.lock().await;
    engine.remove_batch(&task_id)
}

/// Open the containing folder of a file in the system file explorer.
///
/// On Windows: runs `explorer.exe /select,"<path>"`
/// On macOS: runs `open -R "<path>"`
#[tauri::command]
pub async fn open_containing_folder(path: String) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg("/select,")
            .arg(&path)
            .spawn()
            .map_err(|e| AppError::FileSystem {
                message: format!("Failed to open explorer: {}", e),
                path: path.clone(),
            })?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .spawn()
            .map_err(|e| AppError::FileSystem {
                message: format!("Failed to open Finder: {}", e),
                path: path.clone(),
            })?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // On Linux, try xdg-open on the parent directory
        let parent = std::path::Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        std::process::Command::new("xdg-open")
            .arg(&parent)
            .spawn()
            .map_err(|e| AppError::FileSystem {
                message: format!("Failed to open file manager: {}", e),
                path: path.clone(),
            })?;
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Upload Commands
// ─────────────────────────────────────────────────────────────────────────────

/// Upload one or more files to OneDrive/SharePoint.
///
/// Reads each file's size, creates upload tasks for each.
/// Returns a list of task IDs for tracking progress.
#[tauri::command]
pub async fn upload_files(
    cloud_env: String,
    drive_id: String,
    parent_id: String,
    file_paths: Vec<String>,
    auth_module: State<'_, Mutex<AuthModule>>,
    upload_engine: State<'_, Mutex<UploadEngine>>,
) -> Result<Vec<String>, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let base_url = client.base_url().to_string();

    let mut task_ids = Vec::new();
    let mut engine = upload_engine.lock().await;

    for file_path_str in file_paths {
        let file_path = PathBuf::from(&file_path_str);

        let metadata = tokio::fs::metadata(&file_path)
            .await
            .map_err(|e| AppError::FileSystem {
                message: format!("Failed to read file metadata: {}", e),
                path: file_path_str.clone(),
            })?;

        let file_name = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| file_path_str.clone());

        let task_id = engine
            .create_task(UploadParams {
                file_name,
                drive_id: drive_id.clone(),
                parent_id: parent_id.clone(),
                local_path: file_path,
                total_bytes: metadata.len(),
                base_url: base_url.clone(),
                token: token.clone(),
            })
            .await?;

        task_ids.push(task_id);
    }

    Ok(task_ids)
}

/// Upload an entire folder recursively to OneDrive/SharePoint.
///
/// Creates cloud folders mirroring the local directory structure,
/// then enqueues upload tasks for all files found.
/// Returns a list of task IDs for tracking progress.
#[tauri::command]
pub async fn upload_folder(
    cloud_env: String,
    drive_id: String,
    parent_id: String,
    folder_path: String,
    auth_module: State<'_, Mutex<AuthModule>>,
    upload_engine: State<'_, Mutex<UploadEngine>>,
) -> Result<Vec<String>, AppError> {
    let env = parse_cloud_env(&cloud_env)?;
    let token = {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_drive(env.clone(), &drive_id).await?
    };

    let client = GraphClient::new(env);
    let base_url = client.base_url().to_string();
    let http_client = client.http_client();

    let local_folder = PathBuf::from(&folder_path);

    // Get all file upload params by recursively traversing the folder
    let all_params = upload_folder_recursive(
        http_client,
        &base_url,
        &token,
        &drive_id,
        &parent_id,
        &local_folder,
    )
    .await?;

    // Enqueue upload tasks for each file
    let mut task_ids = Vec::new();
    let mut engine = upload_engine.lock().await;

    for params in all_params {
        let task_id = engine.create_task(params).await?;
        task_ids.push(task_id);
    }

    Ok(task_ids)
}

/// Cancel an active upload task.
#[tauri::command]
pub async fn cancel_upload(
    task_id: String,
    upload_engine: State<'_, Mutex<UploadEngine>>,
) -> Result<(), AppError> {
    let mut engine = upload_engine.lock().await;
    engine.cancel_task(&task_id)
}

/// Opens a loopback channel that receives the finished recording from the
/// in-webview stream pipeline (see `src/stream_boot.js`).
///
/// Used when tenant policy blocks Graph downloads (`@microsoft.graph.downloadUrl`
/// withheld) but the preview player can still stream the recording. The webview
/// captures the player's own DASH manifest and encrypted segments, muxes them
/// into a flat MP4, and POSTs the bytes to the returned channel.
#[tauri::command]
pub async fn begin_stream_download(
    save_path: String,
) -> Result<StreamDownloadChannel, AppError> {
    if save_path.trim().is_empty() {
        return Err(AppError::Validation {
            message: "Save path must not be empty".to_string(),
            field: "save_path".to_string(),
        });
    }
    start_upload_server(PathBuf::from(save_path))
        .await
        .map_err(|e| AppError::Transfer {
            message: e,
            task_id: String::new(),
        })
}
