use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::errors::AppError;
use crate::models::ProgressEvent;

/// Files at or below this size use simple PUT upload.
const SIMPLE_UPLOAD_LIMIT: u64 = 4 * 1024 * 1024; // 4 MB

/// Chunk size for session-based uploads (320 KB, required by OneDrive API).
const UPLOAD_CHUNK_SIZE: usize = 320 * 1024; // 320 KB

/// Maximum retry attempts per chunk.
const MAX_RETRIES: u32 = 3;

/// Status of an upload task.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum UploadStatus {
    Queued,
    Uploading,
    Completed,
    Failed(String),
    Cancelled,
}

/// A single upload task tracking file transfer state.
pub struct UploadTask {
    pub id: String,
    pub file_name: String,
    pub drive_id: String,
    pub parent_id: String,
    pub local_path: PathBuf,
    pub total_bytes: u64,
    pub uploaded_bytes: Arc<AtomicU64>,
    pub status: UploadStatus,
    pub cancel_token: CancellationToken,
    pub started_at: Option<Instant>,
}

/// Parameters for creating a new upload task.
pub struct UploadParams {
    pub file_name: String,
    pub drive_id: String,
    pub parent_id: String,
    pub local_path: PathBuf,
    pub total_bytes: u64,
    pub base_url: String,
    pub token: String,
}

/// Response from createUploadSession endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadSessionResponse {
    upload_url: String,
}

/// Request body for creating an upload session.
#[derive(Debug, Serialize)]
struct CreateSessionBody {
    item: SessionItemProperties,
}

/// Item properties for the upload session request.
#[derive(Debug, Serialize)]
struct SessionItemProperties {
    #[serde(rename = "@microsoft.graph.conflictBehavior")]
    conflict_behavior: String,
}

/// The upload engine managing multiple upload tasks.
pub struct UploadEngine {
    tasks: HashMap<String, UploadTask>,
    app_handle: AppHandle,
    http_client: Client,
}

impl UploadEngine {
    /// Create a new UploadEngine.
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            tasks: HashMap::new(),
            app_handle,
            http_client: Client::new(),
        }
    }

    /// Create and start a new upload task.
    ///
    /// Determines strategy based on file size:
    /// - ≤4 MB: Simple PUT upload
    /// - >4 MB: Session-based chunked upload (320 KB chunks)
    pub async fn create_task(&mut self, params: UploadParams) -> Result<String, AppError> {
        let task_id = Uuid::new_v4().to_string();

        let cancel_token = CancellationToken::new();
        let uploaded_bytes = Arc::new(AtomicU64::new(0));

        let task = UploadTask {
            id: task_id.clone(),
            file_name: params.file_name.clone(),
            drive_id: params.drive_id.clone(),
            parent_id: params.parent_id.clone(),
            local_path: params.local_path.clone(),
            total_bytes: params.total_bytes,
            uploaded_bytes: Arc::clone(&uploaded_bytes),
            status: UploadStatus::Uploading,
            cancel_token: cancel_token.clone(),
            started_at: Some(Instant::now()),
        };

        self.tasks.insert(task_id.clone(), task);

        // Spawn the upload worker
        let http_client = self.http_client.clone();
        let app_handle = self.app_handle.clone();
        let task_id_clone = task_id.clone();
        let file_name = params.file_name;
        let total_bytes = params.total_bytes;

        tokio::spawn(async move {
            let result = run_upload(
                http_client,
                app_handle.clone(),
                task_id_clone.clone(),
                file_name.clone(),
                params.drive_id,
                params.parent_id,
                params.local_path,
                total_bytes,
                uploaded_bytes.clone(),
                params.base_url,
                params.token,
                cancel_token,
            )
            .await;

            // Emit final status
            let (status, error) = match &result {
                Ok(()) => ("completed".to_string(), None),
                Err(e) => ("failed".to_string(), Some(e.clone())),
            };

            let _ = app_handle.emit(
                "progress-event",
                ProgressEvent {
                    task_id: task_id_clone,
                    file_name,
                    status,
                    total_bytes,
                    transferred_bytes: uploaded_bytes.load(Ordering::Relaxed),
                    speed_bps: 0,
                    elapsed_secs: 0.0,
                    error,
                    local_path: None,
                },
            );
        });

        Ok(task_id)
    }

    /// Cancel an active upload task.
    pub fn cancel_task(&mut self, task_id: &str) -> Result<(), AppError> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| AppError::Transfer {
                message: "Task not found".to_string(),
                task_id: task_id.to_string(),
            })?;

        task.cancel_token.cancel();
        task.status = UploadStatus::Cancelled;

        // Remove task from the map
        self.tasks.remove(task_id);

        Ok(())
    }

    /// Get the current status of a task.
    pub fn get_task_status(&self, task_id: &str) -> Option<&UploadStatus> {
        self.tasks.get(task_id).map(|t| &t.status)
    }

    /// Get all task IDs.
    pub fn task_ids(&self) -> Vec<String> {
        self.tasks.keys().cloned().collect()
    }
}

/// Run the upload, choosing strategy based on file size.
async fn run_upload(
    http_client: Client,
    app_handle: AppHandle,
    task_id: String,
    file_name: String,
    drive_id: String,
    parent_id: String,
    local_path: PathBuf,
    total_bytes: u64,
    uploaded_bytes: Arc<AtomicU64>,
    base_url: String,
    token: String,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    // Spawn progress reporter (emits at least once per second)
    let progress_cancel = cancel_token.clone();
    let progress_uploaded = Arc::clone(&uploaded_bytes);
    let progress_app_handle = app_handle.clone();
    let progress_task_id = task_id.clone();
    let progress_file_name = file_name.clone();

    let progress_handle = tokio::spawn(async move {
        let mut last_bytes = progress_uploaded.load(Ordering::Relaxed);
        let start_time = Instant::now();

        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                _ = progress_cancel.cancelled() => {
                    break;
                }
            }

            let current_bytes = progress_uploaded.load(Ordering::Relaxed);
            let speed = current_bytes.saturating_sub(last_bytes);
            last_bytes = current_bytes;

            let elapsed = start_time.elapsed().as_secs_f64();

            let status = if current_bytes >= total_bytes {
                "completed"
            } else {
                "uploading"
            };

            let _ = progress_app_handle.emit(
                "progress-event",
                ProgressEvent {
                    task_id: progress_task_id.clone(),
                    file_name: progress_file_name.clone(),
                    status: status.to_string(),
                    total_bytes,
                    transferred_bytes: current_bytes,
                    speed_bps: speed,
                    elapsed_secs: elapsed,
                    error: None,
                    local_path: None,
                },
            );

            if current_bytes >= total_bytes {
                break;
            }
        }
    });

    let result = if total_bytes <= SIMPLE_UPLOAD_LIMIT {
        simple_upload(
            &http_client,
            &base_url,
            &token,
            &drive_id,
            &parent_id,
            &file_name,
            &local_path,
            total_bytes,
            &uploaded_bytes,
            &cancel_token,
        )
        .await
    } else {
        session_upload(
            &http_client,
            &base_url,
            &token,
            &drive_id,
            &parent_id,
            &file_name,
            &local_path,
            total_bytes,
            &uploaded_bytes,
            &cancel_token,
        )
        .await
    };

    // Stop the progress reporter
    cancel_token.cancel();
    let _ = progress_handle.await;

    result
}

/// Simple upload: PUT the entire file content for files ≤4 MB.
async fn simple_upload(
    http_client: &Client,
    base_url: &str,
    token: &str,
    drive_id: &str,
    parent_id: &str,
    file_name: &str,
    local_path: &PathBuf,
    total_bytes: u64,
    uploaded_bytes: &Arc<AtomicU64>,
    cancel_token: &CancellationToken,
) -> Result<(), String> {
    if cancel_token.is_cancelled() {
        return Err("Upload cancelled".to_string());
    }

    // Read the entire file
    let file_content = tokio::fs::read(local_path)
        .await
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let url = format!(
        "{}/drives/{}/items/{}:/{}:/content",
        base_url, drive_id, parent_id, file_name
    );

    let response = http_client
        .put(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/octet-stream")
        .header("@microsoft.graph.conflictBehavior", "rename")
        .body(file_content)
        .send()
        .await
        .map_err(|e| format!("Simple upload request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "Simple upload failed with status {}: {}",
            status, body
        ));
    }

    uploaded_bytes.store(total_bytes, Ordering::Relaxed);

    Ok(())
}

/// Session-based upload: create upload session, then upload in 320 KB chunks.
async fn session_upload(
    http_client: &Client,
    base_url: &str,
    token: &str,
    drive_id: &str,
    parent_id: &str,
    file_name: &str,
    local_path: &PathBuf,
    total_bytes: u64,
    uploaded_bytes: &Arc<AtomicU64>,
    cancel_token: &CancellationToken,
) -> Result<(), String> {
    if cancel_token.is_cancelled() {
        return Err("Upload cancelled".to_string());
    }

    // Step 1: Create upload session
    let session_url = format!(
        "{}/drives/{}/items/{}:/{}:/createUploadSession",
        base_url, drive_id, parent_id, file_name
    );

    let session_body = CreateSessionBody {
        item: SessionItemProperties {
            conflict_behavior: "rename".to_string(),
        },
    };

    let session_response = http_client
        .post(&session_url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&session_body)
        .send()
        .await
        .map_err(|e| format!("Create upload session failed: {}", e))?;

    if !session_response.status().is_success() {
        let status = session_response.status().as_u16();
        let body = session_response.text().await.unwrap_or_default();
        return Err(format!(
            "Create upload session failed with status {}: {}",
            status, body
        ));
    }

    let session: UploadSessionResponse = session_response
        .json()
        .await
        .map_err(|e| format!("Failed to parse upload session response: {}", e))?;

    let upload_url = session.upload_url;

    // Step 2: Upload in chunks
    let mut file = tokio::fs::File::open(local_path)
        .await
        .map_err(|e| format!("Failed to open file for upload: {}", e))?;

    let mut offset: u64 = 0;

    while offset < total_bytes {
        if cancel_token.is_cancelled() {
            return Err("Upload cancelled".to_string());
        }

        let chunk_size = std::cmp::min(UPLOAD_CHUNK_SIZE as u64, total_bytes - offset) as usize;
        let mut buffer = vec![0u8; chunk_size];

        file.read_exact(&mut buffer)
            .await
            .map_err(|e| format!("Failed to read chunk from file: {}", e))?;

        let range_end = offset + chunk_size as u64 - 1;
        let content_range = format!("bytes {}-{}/{}", offset, range_end, total_bytes);

        // Retry logic: up to 3 attempts with exponential backoff
        let mut last_error = String::new();
        let mut success = false;

        for attempt in 0..MAX_RETRIES {
            if cancel_token.is_cancelled() {
                return Err("Upload cancelled".to_string());
            }

            let result = http_client
                .put(&upload_url)
                .header("Content-Range", &content_range)
                .header("Content-Length", chunk_size.to_string())
                .body(buffer.clone())
                .send()
                .await;

            match result {
                Ok(response) => {
                    let status_code = response.status().as_u16();
                    // 200/201 = final chunk complete, 202 = chunk accepted, continue
                    if status_code == 200 || status_code == 201 || status_code == 202 {
                        success = true;
                        break;
                    } else {
                        let body = response.text().await.unwrap_or_default();
                        last_error =
                            format!("Chunk upload failed with status {}: {}", status_code, body);
                    }
                }
                Err(e) => {
                    last_error = format!("Chunk upload request failed: {}", e);
                }
            }

            // Exponential backoff: 1s, 2s, 4s
            if attempt < MAX_RETRIES - 1 {
                let delay = std::time::Duration::from_secs(1 << attempt);
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = cancel_token.cancelled() => {
                        return Err("Upload cancelled".to_string());
                    }
                }
            }
        }

        if !success {
            return Err(format!(
                "Chunk upload failed after {} retries: {}",
                MAX_RETRIES, last_error
            ));
        }

        offset += chunk_size as u64;
        uploaded_bytes.store(offset, Ordering::Relaxed);
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Folder Upload Helper
// ─────────────────────────────────────────────────────────────────────────────

/// Response from creating a folder via Graph API.
#[derive(Debug, Deserialize)]
struct CreateFolderResponse {
    id: String,
}

/// Recursively traverse a local directory, create corresponding cloud folders,
/// and collect `UploadParams` for all files found.
///
/// For each subdirectory: creates a cloud folder via Graph API and recurses.
/// For each file: collects an `UploadParams` entry for the caller to enqueue.
pub async fn upload_folder_recursive(
    http_client: &Client,
    base_url: &str,
    token: &str,
    drive_id: &str,
    parent_id: &str,
    local_folder_path: &Path,
) -> Result<Vec<UploadParams>, AppError> {
    let mut all_params: Vec<UploadParams> = Vec::new();

    let mut read_dir =
        tokio::fs::read_dir(local_folder_path)
            .await
            .map_err(|e| AppError::FileSystem {
                message: format!("Failed to read directory: {}", e),
                path: local_folder_path.to_string_lossy().to_string(),
            })?;

    while let Some(entry) = read_dir
        .next_entry()
        .await
        .map_err(|e| AppError::FileSystem {
            message: format!("Failed to read directory entry: {}", e),
            path: local_folder_path.to_string_lossy().to_string(),
        })?
    {
        let entry_path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();

        let file_type = entry.file_type().await.map_err(|e| AppError::FileSystem {
            message: format!("Failed to get file type: {}", e),
            path: entry_path.to_string_lossy().to_string(),
        })?;

        if file_type.is_dir() {
            // Create a cloud folder via Graph API
            let url = format!(
                "{}/drives/{}/items/{}/children",
                base_url, drive_id, parent_id
            );

            let body = serde_json::json!({
                "name": file_name,
                "folder": {},
                "@microsoft.graph.conflictBehavior": "rename"
            });

            let response = http_client
                .post(&url)
                .header("Authorization", format!("Bearer {}", token))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| AppError::Network {
                    message: format!("Failed to create cloud folder '{}': {}", file_name, e),
                    retryable: true,
                })?;

            if !response.status().is_success() {
                let status_code = response.status().as_u16();
                let error_body = response.text().await.unwrap_or_default();
                return Err(AppError::GraphApi {
                    message: format!("Failed to create folder '{}': {}", file_name, error_body),
                    status_code,
                });
            }

            let folder_resp: CreateFolderResponse =
                response.json().await.map_err(|e| AppError::GraphApi {
                    message: format!(
                        "Failed to parse create folder response for '{}': {}",
                        file_name, e
                    ),
                    status_code: 0,
                })?;

            // Recurse into the subdirectory with the new folder's ID as parent
            let mut sub_params = Box::pin(upload_folder_recursive(
                http_client,
                base_url,
                token,
                drive_id,
                &folder_resp.id,
                &entry_path,
            ))
            .await?;
            all_params.append(&mut sub_params);
        } else if file_type.is_file() {
            // Collect UploadParams for this file
            let metadata =
                tokio::fs::metadata(&entry_path)
                    .await
                    .map_err(|e| AppError::FileSystem {
                        message: format!("Failed to read file metadata: {}", e),
                        path: entry_path.to_string_lossy().to_string(),
                    })?;

            all_params.push(UploadParams {
                file_name,
                drive_id: drive_id.to_string(),
                parent_id: parent_id.to_string(),
                local_path: entry_path,
                total_bytes: metadata.len(),
                base_url: base_url.to_string(),
                token: token.to_string(),
            });
        }
        // Skip symlinks and other special file types
    }

    Ok(all_params)
}
