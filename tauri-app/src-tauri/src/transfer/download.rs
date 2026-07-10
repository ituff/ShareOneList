use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Duration, Utc};
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::errors::AppError;
use crate::models::ProgressEvent;

/// Maximum concurrency for parallel chunk downloads.
const MAX_CONCURRENT_CHUNKS: usize = 8;

/// Maximum chunk size in bytes (1 MB).
const MAX_CHUNK_SIZE: u64 = 1024 * 1024;

/// URL freshness duration: if older than 1 hour, a new URL is needed.
const URL_FRESHNESS_SECS: i64 = 3600;

/// Status of a download task.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum TaskStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed(String),
}

/// State for a single download chunk.
#[derive(Debug, Clone)]
pub struct ChunkState {
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

/// A single download task tracking file transfer state.
pub struct DownloadTask {
    pub id: String,
    pub file_name: String,
    pub drive_id: String,
    pub item_id: String,
    pub local_path: PathBuf,
    pub total_bytes: u64,
    pub downloaded_bytes: Arc<AtomicU64>,
    pub status: TaskStatus,
    pub download_url: String,
    pub url_obtained_at: DateTime<Utc>,
    pub chunks: Vec<ChunkState>,
    pub cancel_token: CancellationToken,
    pub started_at: Option<Instant>,
}

/// Parameters for creating a new download task.
pub struct DownloadParams {
    pub file_name: String,
    pub drive_id: String,
    pub item_id: String,
    pub local_path: PathBuf,
    pub total_bytes: u64,
    pub download_url: String,
}

/// The multi-chunk download engine.
pub struct DownloadEngine {
    tasks: HashMap<String, DownloadTask>,
    app_handle: AppHandle,
    http_client: Client,
}

impl DownloadEngine {
    /// Create a new DownloadEngine.
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            tasks: HashMap::new(),
            app_handle,
            http_client: Client::new(),
        }
    }

    /// Create a new download task.
    ///
    /// Calculates chunks, resolves file name conflicts, and spawns the download.
    pub async fn create_task(&mut self, params: DownloadParams) -> Result<String, AppError> {
        let task_id = Uuid::new_v4().to_string();

        // Resolve file name conflicts
        let local_path = resolve_conflict(&params.local_path);

        // Calculate chunks
        let chunks = calculate_chunks(params.total_bytes);

        let cancel_token = CancellationToken::new();
        let downloaded_bytes = Arc::new(AtomicU64::new(0));

        let task = DownloadTask {
            id: task_id.clone(),
            file_name: params.file_name.clone(),
            drive_id: params.drive_id,
            item_id: params.item_id,
            local_path: local_path.clone(),
            total_bytes: params.total_bytes,
            downloaded_bytes: Arc::clone(&downloaded_bytes),
            status: TaskStatus::Downloading,
            download_url: params.download_url.clone(),
            url_obtained_at: Utc::now(),
            chunks: chunks.clone(),
            cancel_token: cancel_token.clone(),
            started_at: Some(Instant::now()),
        };

        self.tasks.insert(task_id.clone(), task);

        // Spawn the download worker
        let http_client = self.http_client.clone();
        let app_handle = self.app_handle.clone();
        let file_name = params.file_name;
        let download_url = params.download_url;
        let total_bytes = params.total_bytes;
        let task_id_clone = task_id.clone();

        tokio::spawn(async move {
            let result = run_download(
                http_client,
                app_handle.clone(),
                task_id_clone.clone(),
                file_name.clone(),
                download_url,
                local_path,
                chunks,
                total_bytes,
                downloaded_bytes,
                cancel_token,
            )
            .await;

            // Emit final status
            let status = match &result {
                Ok(()) => "completed".to_string(),
                Err(e) => format!("failed: {}", e),
            };

            let _ = app_handle.emit(
                "progress-event",
                ProgressEvent {
                    task_id: task_id_clone,
                    file_name,
                    status,
                    total_bytes,
                    transferred_bytes: total_bytes,
                    speed_bps: 0,
                    elapsed_secs: 0.0,
                    error: result.err().map(|e| e.to_string()),
                },
            );
        });

        Ok(task_id)
    }

    /// Pause an active download task.
    pub fn pause_task(&mut self, task_id: &str) -> Result<(), AppError> {
        let task = self.tasks.get_mut(task_id).ok_or_else(|| AppError::Transfer {
            message: "Task not found".to_string(),
            task_id: task_id.to_string(),
        })?;

        if task.status != TaskStatus::Downloading {
            return Err(AppError::Transfer {
                message: "Task is not currently downloading".to_string(),
                task_id: task_id.to_string(),
            });
        }

        // Signal cancellation to stop chunk downloads
        task.cancel_token.cancel();
        task.status = TaskStatus::Paused;

        // Record current progress in chunks
        let total_downloaded = task.downloaded_bytes.load(Ordering::Relaxed);
        update_chunk_progress(&mut task.chunks, total_downloaded);

        // Emit paused event
        let _ = self.app_handle.emit(
            "progress-event",
            ProgressEvent {
                task_id: task_id.to_string(),
                file_name: task.file_name.clone(),
                status: "paused".to_string(),
                total_bytes: task.total_bytes,
                transferred_bytes: total_downloaded,
                speed_bps: 0,
                elapsed_secs: task
                    .started_at
                    .map(|s| s.elapsed().as_secs_f64())
                    .unwrap_or(0.0),
                error: None,
            },
        );

        Ok(())
    }

    /// Resume a paused download task.
    ///
    /// If the download URL is older than 1 hour, `new_url` must be provided.
    pub async fn resume_task(
        &mut self,
        task_id: &str,
        new_url: Option<String>,
    ) -> Result<(), AppError> {
        let task = self.tasks.get_mut(task_id).ok_or_else(|| AppError::Transfer {
            message: "Task not found".to_string(),
            task_id: task_id.to_string(),
        })?;

        if task.status != TaskStatus::Paused {
            return Err(AppError::Transfer {
                message: "Task is not paused".to_string(),
                task_id: task_id.to_string(),
            });
        }

        // Check URL freshness
        let elapsed = Utc::now() - task.url_obtained_at;
        if elapsed > Duration::seconds(URL_FRESHNESS_SECS) {
            match new_url {
                Some(url) => {
                    task.download_url = url;
                    task.url_obtained_at = Utc::now();
                }
                None => {
                    return Err(AppError::Transfer {
                        message: "Download URL has expired (>1 hour). A fresh URL is required."
                            .to_string(),
                        task_id: task_id.to_string(),
                    });
                }
            }
        } else if let Some(url) = new_url {
            task.download_url = url;
            task.url_obtained_at = Utc::now();
        }

        // Reset cancel token for the new download session
        let new_cancel_token = CancellationToken::new();
        task.cancel_token = new_cancel_token.clone();
        task.status = TaskStatus::Downloading;
        task.started_at = Some(Instant::now());

        // Build remaining chunks from current progress
        let chunks = task.chunks.clone();
        let download_url = task.download_url.clone();
        let local_path = task.local_path.clone();
        let total_bytes = task.total_bytes;
        let downloaded_bytes = Arc::clone(&task.downloaded_bytes);
        let file_name = task.file_name.clone();
        let task_id_clone = task_id.to_string();
        let http_client = self.http_client.clone();
        let app_handle = self.app_handle.clone();

        tokio::spawn(async move {
            let result = run_download(
                http_client,
                app_handle.clone(),
                task_id_clone.clone(),
                file_name.clone(),
                download_url,
                local_path,
                chunks,
                total_bytes,
                downloaded_bytes,
                new_cancel_token,
            )
            .await;

            let status = match &result {
                Ok(()) => "completed".to_string(),
                Err(e) => format!("failed: {}", e),
            };

            let _ = app_handle.emit(
                "progress-event",
                ProgressEvent {
                    task_id: task_id_clone,
                    file_name,
                    status,
                    total_bytes,
                    transferred_bytes: total_bytes,
                    speed_bps: 0,
                    elapsed_secs: 0.0,
                    error: result.err().map(|e| e.to_string()),
                },
            );
        });

        Ok(())
    }

    /// Cancel a download task. Aborts the transfer and deletes partial files.
    pub fn cancel_task(&mut self, task_id: &str) -> Result<(), AppError> {
        let task = self.tasks.get(task_id).ok_or_else(|| AppError::Transfer {
            message: "Task not found".to_string(),
            task_id: task_id.to_string(),
        })?;

        // Signal cancellation
        task.cancel_token.cancel();

        // Delete partial file
        let local_path = task.local_path.clone();
        if local_path.exists() {
            let _ = std::fs::remove_file(&local_path);
        }

        // Remove task from the map
        self.tasks.remove(task_id);

        Ok(())
    }

    /// Get the current status of a task.
    pub fn get_task_status(&self, task_id: &str) -> Option<&TaskStatus> {
        self.tasks.get(task_id).map(|t| &t.status)
    }

    /// Get all task IDs.
    pub fn task_ids(&self) -> Vec<String> {
        self.tasks.keys().cloned().collect()
    }

    /// Check if a download URL needs refreshing (older than 1 hour).
    pub fn url_needs_refresh(&self, task_id: &str) -> Option<bool> {
        self.tasks.get(task_id).map(|t| {
            let elapsed = Utc::now() - t.url_obtained_at;
            elapsed > Duration::seconds(URL_FRESHNESS_SECS)
        })
    }
}

/// Calculate download chunks for a given file size.
///
/// Divides total_bytes into up to 8 chunks, each at most 1 MB.
/// For files smaller than 8 MB, fewer chunks are used.
pub fn calculate_chunks(total_bytes: u64) -> Vec<ChunkState> {
    if total_bytes == 0 {
        return vec![ChunkState {
            start: 0,
            end: 0,
            downloaded: 0,
        }];
    }

    // Determine chunk count: up to 8 chunks, each at most MAX_CHUNK_SIZE
    let num_chunks = if total_bytes <= MAX_CHUNK_SIZE {
        1
    } else {
        let needed = (total_bytes + MAX_CHUNK_SIZE - 1) / MAX_CHUNK_SIZE;
        needed.min(8) as usize
    };

    let chunk_size = total_bytes / num_chunks as u64;
    let remainder = total_bytes % num_chunks as u64;

    let mut chunks = Vec::with_capacity(num_chunks);
    let mut offset = 0u64;

    for i in 0..num_chunks {
        // Distribute remainder across first chunks
        let size = if (i as u64) < remainder {
            chunk_size + 1
        } else {
            chunk_size
        };
        let end = offset + size - 1;
        chunks.push(ChunkState {
            start: offset,
            end,
            downloaded: 0,
        });
        offset = end + 1;
    }

    chunks
}

/// Resolve file name conflicts by appending " (1)", " (2)", etc.
fn resolve_conflict(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = path.extension().and_then(|e| e.to_str());
    let parent = path.parent().unwrap_or(Path::new("."));

    for i in 1..u32::MAX {
        let new_name = match ext {
            Some(e) => format!("{} ({}).{}", stem, i, e),
            None => format!("{} ({})", stem, i),
        };
        let candidate = parent.join(&new_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    // Fallback (extremely unlikely)
    path.to_path_buf()
}

/// Update chunk progress based on total downloaded bytes.
fn update_chunk_progress(chunks: &mut [ChunkState], total_downloaded: u64) {
    let mut remaining = total_downloaded;
    for chunk in chunks.iter_mut() {
        let chunk_total = chunk.end - chunk.start + 1;
        if remaining >= chunk_total {
            chunk.downloaded = chunk_total;
            remaining -= chunk_total;
        } else {
            chunk.downloaded = remaining;
            remaining = 0;
        }
    }
}

/// Run the actual download, downloading all chunks in parallel.
async fn run_download(
    http_client: Client,
    app_handle: AppHandle,
    task_id: String,
    file_name: String,
    download_url: String,
    local_path: PathBuf,
    chunks: Vec<ChunkState>,
    total_bytes: u64,
    downloaded_bytes: Arc<AtomicU64>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    // Create or open the output file
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&local_path)
        .await
        .map_err(|e| format!("Failed to open output file: {}", e))?;

    // Pre-allocate the file to the full size
    file.set_len(total_bytes)
        .await
        .map_err(|e| format!("Failed to set file size: {}", e))?;

    drop(file);

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CHUNKS));
    let mut handles = Vec::new();

    // Progress tracking for speed calculation
    let progress_cancel = cancel_token.clone();
    let progress_downloaded = Arc::clone(&downloaded_bytes);
    let progress_app_handle = app_handle.clone();
    let progress_task_id = task_id.clone();
    let progress_file_name = file_name.clone();

    // Spawn progress reporter (emits at least once per second)
    let progress_handle = tokio::spawn(async move {
        let mut last_bytes = progress_downloaded.load(Ordering::Relaxed);
        let start_time = Instant::now();

        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                _ = progress_cancel.cancelled() => {
                    break;
                }
            }

            let current_bytes = progress_downloaded.load(Ordering::Relaxed);
            let speed = current_bytes.saturating_sub(last_bytes);
            last_bytes = current_bytes;

            let elapsed = start_time.elapsed().as_secs_f64();

            let status = if current_bytes >= total_bytes {
                "completed"
            } else {
                "downloading"
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
                },
            );

            if current_bytes >= total_bytes {
                break;
            }
        }
    });

    // Spawn chunk download tasks
    for chunk in chunks {
        // Skip already completed chunks (for resume)
        if chunk.downloaded >= (chunk.end - chunk.start + 1) {
            continue;
        }

        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| format!("Semaphore error: {}", e))?;

        let client = http_client.clone();
        let url = download_url.clone();
        let path = local_path.clone();
        let token = cancel_token.clone();
        let bytes_counter = Arc::clone(&downloaded_bytes);

        let actual_start = chunk.start + chunk.downloaded;
        let end = chunk.end;

        let handle = tokio::spawn(async move {
            let _permit = permit;
            download_chunk(client, url, path, actual_start, end, bytes_counter, token).await
        });

        handles.push(handle);
    }

    // Wait for all chunk tasks to complete
    let mut error: Option<String> = None;
    for handle in handles {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                if error.is_none() {
                    error = Some(e);
                }
            }
            Err(e) => {
                if error.is_none() {
                    error = Some(format!("Task join error: {}", e));
                }
            }
        }
    }

    // Stop the progress reporter
    cancel_token.cancel();
    let _ = progress_handle.await;

    if let Some(err) = error {
        // If it was a cancellation (pause/cancel), don't report as error
        if err.contains("cancelled") {
            return Ok(());
        }
        return Err(err);
    }

    Ok(())
}

/// Download a single chunk using Range headers.
async fn download_chunk(
    client: Client,
    url: String,
    path: PathBuf,
    start: u64,
    end: u64,
    downloaded_bytes: Arc<AtomicU64>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    let range = format!("bytes={}-{}", start, end);

    let response = client
        .get(&url)
        .header("Range", &range)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if !response.status().is_success() && response.status().as_u16() != 206 {
        return Err(format!(
            "Unexpected status code: {}",
            response.status().as_u16()
        ));
    }

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .await
        .map_err(|e| format!("Failed to open file for writing: {}", e))?;

    file.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(|e| format!("Failed to seek: {}", e))?;

    let mut stream = response.bytes_stream();

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                return Err("Download cancelled".to_string());
            }
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(bytes)) => {
                        file.write_all(&bytes).await.map_err(|e| format!("Write error: {}", e))?;
                        downloaded_bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                    }
                    Some(Err(e)) => {
                        return Err(format!("Stream error: {}", e));
                    }
                    None => {
                        // Stream complete
                        break;
                    }
                }
            }
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("Flush error: {}", e))?;

    Ok(())
}


// ─────────────────────────────────────────────────────────────────────────────
// Folder Download (Recursive Traversal)
// ─────────────────────────────────────────────────────────────────────────────

/// A minimal representation of a drive item returned by the Graph API children endpoint.
/// Used internally by `download_folder_recursive` to avoid coupling with the commands module.
#[derive(Debug, Deserialize)]
struct FolderChildItem {
    id: Option<String>,
    name: Option<String>,
    size: Option<u64>,
    folder: Option<serde_json::Value>,
    #[serde(rename = "@microsoft.graph.downloadUrl")]
    download_url: Option<String>,
}

/// Graph API collection response for folder children listing.
#[derive(Debug, Deserialize)]
struct FolderChildrenResponse {
    value: Vec<FolderChildItem>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

/// Recursively traverses a cloud folder and collects all files as `DownloadParams`.
///
/// This function:
/// 1. Lists the children of `folder_id` via Graph API (with pagination via `@odata.nextLink`)
/// 2. For each child that is a folder, creates the corresponding local directory and recurses
/// 3. For each child that is a file with a download URL, creates a `DownloadParams` entry
///
/// The caller (typically the Tauri command layer) can then enqueue all returned `DownloadParams`
/// into the `DownloadEngine` via `create_task()`.
///
/// # Arguments
/// * `http_client` - A `reqwest::Client` for making HTTP requests
/// * `base_url` - The Graph API base URL (e.g. `https://graph.microsoft.com/v1.0`)
/// * `token` - A valid OAuth2 bearer token
/// * `drive_id` - The drive ID containing the folder
/// * `folder_id` - The item ID of the folder to download
/// * `local_base_path` - The local directory path where the folder contents will be placed
///
/// # Returns
/// A `Vec<DownloadParams>` containing an entry for every file found in the folder hierarchy.
///
/// # Errors
/// Returns `AppError` if the API request fails, JSON parsing fails, or local directory creation fails.
pub async fn download_folder_recursive(
    http_client: &Client,
    base_url: &str,
    token: &str,
    drive_id: &str,
    folder_id: &str,
    local_base_path: &Path,
) -> Result<Vec<DownloadParams>, AppError> {
    // Ensure the local base directory exists
    tokio::fs::create_dir_all(local_base_path)
        .await
        .map_err(|e| AppError::FileSystem {
            message: format!("Failed to create directory: {}", e),
            path: local_base_path.display().to_string(),
        })?;

    let mut all_params: Vec<DownloadParams> = Vec::new();

    // Fetch all children with pagination
    let mut url = format!(
        "{}/drives/{}/items/{}/children?$top=200",
        base_url, drive_id, folder_id
    );

    loop {
        let response = http_client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| AppError::Network {
                message: format!("Failed to list folder children: {}", e),
                retryable: e.is_timeout() || e.is_connect(),
            })?;

        let status = response.status();
        if !status.is_success() {
            let status_code = status.as_u16();
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| format!("HTTP {}", status_code));
            return Err(AppError::GraphApi {
                message,
                status_code,
            });
        }

        let collection: FolderChildrenResponse =
            response.json().await.map_err(|e| AppError::GraphApi {
                message: format!("Failed to parse folder children response: {}", e),
                status_code: 0,
            })?;

        for item in collection.value {
            let item_name = item.name.unwrap_or_default();
            if item_name.is_empty() {
                continue;
            }

            let child_path = local_base_path.join(&item_name);

            if item.folder.is_some() {
                // It's a subfolder — recurse into it
                let item_id = item.id.unwrap_or_default();
                if item_id.is_empty() {
                    continue;
                }

                let sub_params = Box::pin(download_folder_recursive(
                    http_client,
                    base_url,
                    token,
                    drive_id,
                    &item_id,
                    &child_path,
                ))
                .await?;

                all_params.extend(sub_params);
            } else {
                // It's a file — collect download params
                let item_id = item.id.unwrap_or_default();
                let download_url = item.download_url.unwrap_or_default();

                if item_id.is_empty() || download_url.is_empty() {
                    continue;
                }

                all_params.push(DownloadParams {
                    file_name: item_name,
                    drive_id: drive_id.to_string(),
                    item_id,
                    local_path: child_path,
                    total_bytes: item.size.unwrap_or(0),
                    download_url,
                });
            }
        }

        // Follow pagination
        match collection.next_link {
            Some(next) => url = next,
            None => break,
        }
    }

    Ok(all_params)
}
