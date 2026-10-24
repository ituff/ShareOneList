use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Duration, Utc};
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::errors::AppError;
use crate::models::{FailedFile, ProgressEvent};

/// Maximum concurrency for parallel chunk downloads.
const MAX_CONCURRENT_CHUNKS: usize = 8;

/// Maximum number of files downloading at the same time across all batches.
/// Unbounded per-file fan-out (hundreds of streams) got connections reset by
/// the server; capping files keeps each stream's chunk parallelism intact.
const MAX_CONCURRENT_FILES: usize = 6;

/// Maximum chunk size in bytes (1 MB).
const MAX_CHUNK_SIZE: u64 = 1024 * 1024;

/// Status of a download task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed(String),
}

/// State for a single download chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkState {
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

/// A single download task tracking file transfer state.
pub struct DownloadTask {
    pub id: String,
    pub file_name: String,
    pub home_account_id: String,
    pub drive_id: String,
    pub item_id: String,
    pub local_path: PathBuf,
    pub total_bytes: u64,
    pub downloaded_bytes: Arc<AtomicU64>,
    pub status: TaskStatus,
    pub cloud_env: String,
    pub download_url: String,
    pub bearer_token: Option<String>,
    pub url_obtained_at: DateTime<Utc>,
    pub chunks: Arc<Mutex<Vec<ChunkState>>>,
    pub cancel_token: CancellationToken,
    pub started_at: Option<Instant>,
    pub batch_id: String,
    pub etag: Option<String>,
    pub persist_path: PathBuf,
    /// Path bytes are actually written to (`<name>.part`), so the final file
    /// only appears once the download completes. `None` means legacy in-place
    /// writing (also used when a partial could not be migrated).
    pub download_path: Option<PathBuf>,
    /// Expected quickXorHash from Graph; verified after all chunks land.
    pub quick_xor_hash: Option<String>,
}

impl DownloadTask {
    /// The path chunk writes go to.
    pub fn write_path(&self) -> PathBuf {
        self.download_path
            .clone()
            .unwrap_or_else(|| self.local_path.clone())
    }
}

/// Temp file name for a download: ends in `.tmp` because the OneDrive sync
/// client hard-excludes that extension — otherwise it scans and uploads the
/// growing file, holding locks that break our writes. A final name that is
/// itself `.tmp` falls back to the synced `.part` suffix.
fn derive_write_path(final_path: &Path) -> PathBuf {
    let ends_with_tmp = final_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("tmp"))
        .unwrap_or(false);
    let mut os = final_path.as_os_str().to_os_string();
    os.push(if ends_with_tmp { ".part" } else { ".tmp" });
    PathBuf::from(os)
}

/// Parameters for creating a new download task.
pub struct DownloadParams {
    pub file_name: String,
    pub home_account_id: String,
    pub cloud_env: String,
    pub drive_id: String,
    pub item_id: String,
    pub local_path: PathBuf,
    pub total_bytes: u64,
    pub download_url: String,
    pub bearer_token: Option<String>,
    /// Cloud-side quickXorHash for post-download integrity verification.
    /// `None` falls back to size-only verification.
    pub quick_xor_hash: Option<String>,
}

/// A persisted download task snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTask {
    pub id: String,
    pub file_name: String,
    pub home_account_id: String,
    pub drive_id: String,
    pub item_id: String,
    pub local_path: String,
    pub total_bytes: u64,
    pub status: String,
    pub cloud_env: String,
    pub url_obtained_at: String,
    pub chunks: Vec<PersistedChunk>,
    pub batch_id: String,
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_xor_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedChunk {
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

/// A persisted batch task metadata file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedBatch {
    pub id: String,
    pub name: String,
    pub task_ids: Vec<String>,
    pub total_bytes: u64,
    pub home_account_id: String,
    pub local_path: String,
    pub cloud_env: String,
    pub drive_id: String,
    pub url_obtained_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_files: Vec<FailedFile>,
}

/// Batch task result returned to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchInfo {
    pub batch_id: String,
    pub batch_name: String,
}

/// Batch task snapshot returned to the frontend on startup restore.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSnapshot {
    pub id: String,
    pub name: String,
    pub status: String,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub speed_bps: u64,
    pub elapsed_secs: f64,
    pub error: Option<String>,
    pub local_path: String,
    pub cloud_env: String,
    pub drive_id: String,
    pub home_account_id: String,
    pub failed_files: Vec<FailedFile>,
}

/// Result of verifying one downloaded file against the cloud copy.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileVerifyResult {
    pub file_name: String,
    /// "verified" | "size-mismatch" | "hash-mismatch" | "missing"
    /// | "not-downloaded" | "hash-unavailable"
    pub status: String,
    pub detail: Option<String>,
}

/// One downloaded file to verify, cloned out of the engine so the lock is
/// never held across file IO.
#[derive(Debug, Clone)]
pub struct BatchFileRecord {
    pub file_name: String,
    pub local_path: String,
    pub total_bytes: u64,
    pub quick_xor_hash: Option<String>,
    pub item_id: String,
    pub drive_id: String,
    pub status: String,
}

/// Shared batch progress passed into each child download worker.
#[derive(Debug, Clone)]
pub struct BatchProgress {
    pub id: String,
    pub name: String,
    pub total_bytes: u64,
    pub downloaded_bytes: Arc<AtomicU64>,
}

/// The multi-chunk download engine.
pub struct DownloadEngine {
    tasks: HashMap<String, DownloadTask>,
    batches: HashMap<String, DownloadBatch>,
    app_handle: AppHandle,
    http_client: Client,
    state_dir: PathBuf,
    /// Caps how many files run concurrently across all batches (per-file
    /// chunk parallelism is unchanged).
    file_permits: Arc<Semaphore>,
}

/// In-memory batch task tracking multiple file downloads.
#[derive(Clone)]
pub struct DownloadBatch {
    pub id: String,
    pub name: String,
    pub task_ids: Vec<String>,
    pub total_bytes: u64,
    pub home_account_id: String,
    pub downloaded_bytes: Arc<AtomicU64>,
    pub status: TaskStatus,
    pub error: Option<String>,
    /// Child files that failed, with their real error messages. Entries are
    /// removed as the corresponding files succeed on a resumed run.
    pub failed_files: Vec<FailedFile>,
    pub local_path: PathBuf,
    pub cloud_env: String,
    pub drive_id: String,
    pub url_obtained_at: DateTime<Utc>,
}

impl DownloadEngine {
    /// Create a new DownloadEngine.
    pub fn new(app_handle: AppHandle, state_dir: PathBuf) -> Self {
        let tasks_dir = state_dir.join("tasks");
        let batches_dir = state_dir.join("batches");
        let _ = fs::create_dir_all(&tasks_dir);
        let _ = fs::create_dir_all(&batches_dir);

        let (persisted_tasks, persisted_batches) = load_state(&state_dir);
        let mut tasks = HashMap::new();
        let mut batches = HashMap::new();

        for persisted in persisted_tasks {
            let task = restore_task(persisted, &tasks_dir);
            tasks.insert(task.id.clone(), task);
        }

        for persisted in persisted_batches {
            let task_ids = persisted.task_ids.clone();
            let downloaded = task_ids.iter().fold(0u64, |sum, id| {
                sum + tasks
                    .get(id)
                    .map(|t| chunk_downloaded_sum(&t.chunks))
                    .unwrap_or(0)
            });
            let status = compute_batch_status(&tasks, &task_ids);
            let batch = DownloadBatch {
                id: persisted.id.clone(),
                name: persisted.name,
                task_ids,
                total_bytes: persisted.total_bytes,
                home_account_id: persisted.home_account_id,
                downloaded_bytes: Arc::new(AtomicU64::new(downloaded)),
                status,
                error: None,
                failed_files: persisted.failed_files,
                local_path: PathBuf::from(persisted.local_path),
                cloud_env: persisted.cloud_env,
                drive_id: persisted.drive_id,
                url_obtained_at: Utc::now(),
            };
            batches.insert(persisted.id, batch);
        }

        Self {
            tasks,
            batches,
            app_handle,
            http_client: Client::new(),
            state_dir,
            file_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_FILES)),
        }
    }

    /// Create a new batch download containing one or more file tasks.
    pub async fn create_batch(
        &mut self,
        batch_name: String,
        params: Vec<DownloadParams>,
    ) -> Result<BatchInfo, AppError> {
        if params.is_empty() {
            return Err(AppError::Transfer {
                message: "No files to download".to_string(),
                task_id: String::new(),
            });
        }

        let batch_id = Uuid::new_v4().to_string();
        let total_bytes = params.iter().map(|p| p.total_bytes).sum();
        let first = &params[0];
        let local_path = if params.len() == 1 {
            params[0].local_path.clone()
        } else {
            params[0]
                .local_path
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf()
        };
        let batch_downloaded = Arc::new(AtomicU64::new(0));

        self.batches.insert(
            batch_id.clone(),
            DownloadBatch {
                id: batch_id.clone(),
                name: batch_name.clone(),
                task_ids: Vec::new(),
                total_bytes,
                home_account_id: first.home_account_id.clone(),
                downloaded_bytes: Arc::clone(&batch_downloaded),
                status: TaskStatus::Downloading,
                error: None,
                failed_files: Vec::new(),
                local_path,
                cloud_env: first.cloud_env.clone(),
                drive_id: first.drive_id.clone(),
                url_obtained_at: Utc::now(),
            },
        );

        let mut task_ids = Vec::with_capacity(params.len());
        for param in params {
            let task_id = self.create_task_internal(param, &batch_id).await?;
            task_ids.push(task_id);
        }

        if let Some(batch) = self.batches.get_mut(&batch_id) {
            batch.task_ids = task_ids;
        }
        if let Some(batch) = self.batches.get(&batch_id) {
            self.save_batch(batch);
        }

        Ok(BatchInfo {
            batch_id,
            batch_name,
        })
    }

    /// Create a single child task and start its download worker.
    async fn create_task_internal(
        &mut self,
        params: DownloadParams,
        batch_id: &str,
    ) -> Result<String, AppError> {
        let task_id = Uuid::new_v4().to_string();
        let local_path = resolve_conflict(&params.local_path);
        let download_path = Some(derive_write_path(&local_path));
        let chunks = Arc::new(Mutex::new(calculate_chunks(params.total_bytes)));
        let cancel_token = CancellationToken::new();
        let downloaded_bytes = Arc::new(AtomicU64::new(0));
        let persist_path = self.state_dir.join("tasks").join(format!("{}.json", task_id));

        let task = DownloadTask {
            id: task_id.clone(),
            file_name: params.file_name.clone(),
            home_account_id: params.home_account_id.clone(),
            drive_id: params.drive_id.clone(),
            item_id: params.item_id.clone(),
            local_path: local_path.clone(),
            total_bytes: params.total_bytes,
            downloaded_bytes: Arc::clone(&downloaded_bytes),
            status: TaskStatus::Downloading,
            cloud_env: params.cloud_env.clone(),
            download_url: params.download_url.clone(),
            bearer_token: params.bearer_token.clone(),
            url_obtained_at: Utc::now(),
            chunks: Arc::clone(&chunks),
            cancel_token: cancel_token.clone(),
            started_at: Some(Instant::now()),
            batch_id: batch_id.to_string(),
            etag: None,
            persist_path: persist_path.clone(),
            download_path,
            quick_xor_hash: params.quick_xor_hash.clone(),
        };

        self.tasks.insert(task_id.clone(), task);

        let batch_progress = self
            .batches
            .get(batch_id)
            .map(|batch| BatchProgress {
                id: batch.id.clone(),
                name: batch.name.clone(),
                total_bytes: batch.total_bytes,
                downloaded_bytes: Arc::clone(&batch.downloaded_bytes),
            });

        self.spawn_child_download(&task_id, batch_progress);
        self.persist_task_now(&task_id).await;

        Ok(task_id)
    }

    /// Spawn the actual download worker for a child task.
    fn spawn_child_download(&self, task_id: &str, batch_progress: Option<BatchProgress>) {
        let Some(task) = self.tasks.get(task_id) else {
            return;
        };

        let http_client = self.http_client.clone();
        let app_handle = self.app_handle.clone();
        let task_id = task.id.clone();
        let batch_id = task.batch_id.clone();
        let file_name = task.file_name.clone();
        let download_url = task.download_url.clone();
        let bearer_token = task.bearer_token.clone();
        let local_path = task.local_path.clone();
        let total_bytes = task.total_bytes;
        let chunks = Arc::clone(&task.chunks);
        let downloaded_bytes = Arc::clone(&task.downloaded_bytes);
        let cancel_token = task.cancel_token.clone();
        let persist_path = task.persist_path.clone();
        let seed = build_persisted_seed(task);
        let file_permits = Arc::clone(&self.file_permits);
        let write_path = task.write_path();
        let quick_xor_hash = task.quick_xor_hash.clone();

        tokio::spawn(async move {
            // Wait for a file-level slot; a batch paused while queued is
            // cancelled before it ever starts downloading.
            let result = if cancel_token.is_cancelled() {
                Err("Download cancelled".to_string())
            } else {
                match Semaphore::acquire_owned(file_permits).await {
                    Ok(permit) => {
                        let result = run_download(
                            http_client,
                            app_handle.clone(),
                            batch_progress.clone(),
                            task_id.clone(),
                            file_name.clone(),
                            download_url,
                            bearer_token,
                            write_path,
                            local_path.clone(),
                            quick_xor_hash,
                            total_bytes,
                            chunks,
                            downloaded_bytes,
                            cancel_token,
                            persist_path,
                            seed,
                        )
                        .await;
                        drop(permit);
                        result
                    }
                    // The semaphore is never closed; treat a close as cancellation.
                    Err(_) => Err("Download cancelled".to_string()),
                }
            };

            let local_path_display = local_path.to_string_lossy().to_string();
            let (status, error) = match &result {
                Ok(()) => ("completed".to_string(), None),
                Err(e) if e.contains("cancelled") => ("paused".to_string(), None),
                Err(e) => ("failed".to_string(), Some(e.clone())),
            };

            let mut emitted_status = status.clone();
            let mut emit_error = error;
            let mut emit_failed_files = None;
            let mut task_exists = true;
            if let Some(engine_state) = app_handle.try_state::<Mutex<DownloadEngine>>() {
                let mut engine = engine_state.lock().await;
                task_exists = engine.tasks.contains_key(&task_id);
                if let Some(task) = engine.tasks.get_mut(&task_id) {
                    task.status = task_status_from_str(&status).unwrap_or(TaskStatus::Failed(String::new()));
                    engine.persist_task_now(&task_id).await;
                }
                match &result {
                    Ok(()) => engine.record_file_success(&batch_id, &file_name),
                    Err(e) if !e.contains("cancelled") => {
                        log::warn!(
                            "download failed: file={} batch={} error={}",
                            file_name,
                            batch_id,
                            e
                        );
                        engine.record_file_failure(&batch_id, &file_name, e);
                    }
                    _ => {}
                }
                engine.update_batch_status(&batch_id).await;
                emitted_status = engine
                    .batches
                    .get(&batch_id)
                    .map(|batch| task_status_string(&batch.status))
                    .unwrap_or(status.clone());
                if emitted_status == "failed" {
                    emit_error = engine
                        .batches
                        .get(&batch_id)
                        .and_then(|batch| batch.error.clone())
                        .or(emit_error);
                } else {
                    emit_error = None;
                }
                emit_failed_files = engine
                    .batches
                    .get(&batch_id)
                    .map(|batch| batch.failed_files.clone())
                    .filter(|files| !files.is_empty());
            }

            // A cancelled batch removes its tasks from the engine; skip the stale paused event.
            if !task_exists && emitted_status == "paused" {
                return;
            }

            let emit_id = batch_progress
                .as_ref()
                .map(|b| b.id.clone())
                .unwrap_or_else(|| task_id.clone());
            let emit_name = batch_progress
                .as_ref()
                .map(|b| b.name.clone())
                .unwrap_or_else(|| file_name.clone());
            let emit_total = batch_progress
                .as_ref()
                .map(|b| b.total_bytes)
                .unwrap_or(total_bytes);
            let emit_downloaded = batch_progress
                .as_ref()
                .map(|b| b.downloaded_bytes.load(Ordering::Relaxed))
                .unwrap_or(total_bytes);
            let _ = app_handle.emit(
                "progress-event",
                ProgressEvent {
                    task_id: emit_id,
                    file_name: emit_name,
                    status: emitted_status,
                    total_bytes: emit_total,
                    transferred_bytes: emit_downloaded,
                    speed_bps: 0,
                    elapsed_secs: 0.0,
                    error: emit_error,
                    local_path: Some(local_path_display),
                    failed_files: emit_failed_files,
                },
            );
        });
    }

    /// Pause a batch and all active child downloads.
    pub async fn pause_batch(&mut self, batch_id: &str) -> Result<(), AppError> {
        let (name, total_bytes, local_path, task_ids) = {
            let batch = self
                .batches
                .get_mut(batch_id)
                .ok_or_else(|| AppError::Transfer {
                    message: "Task not found".to_string(),
                    task_id: batch_id.to_string(),
                })?;

            if batch.status != TaskStatus::Downloading {
                return Err(AppError::Transfer {
                    message: "Task is not currently downloading".to_string(),
                    task_id: batch_id.to_string(),
                });
            }

            (
                batch.name.clone(),
                batch.total_bytes,
                batch.local_path.clone(),
                batch.task_ids.clone(),
            )
        };

        for task_id in &task_ids {
            if let Some(task) = self.tasks.get_mut(task_id) {
                task.cancel_token.cancel();
                task.status = TaskStatus::Paused;
                self.persist_task_now(task_id).await;
            }
        }

        let downloaded = self
            .batches
            .get(batch_id)
            .map(|b| b.downloaded_bytes.load(Ordering::Relaxed))
            .unwrap_or(0);
        if let Some(batch) = self.batches.get_mut(batch_id) {
            batch.status = TaskStatus::Paused;
        }

        let _ = self.app_handle.emit(
            "progress-event",
            ProgressEvent {
                task_id: batch_id.to_string(),
                file_name: name,
                status: "paused".to_string(),
                total_bytes,
                transferred_bytes: downloaded,
                speed_bps: 0,
                elapsed_secs: 0.0,
                error: None,
                local_path: Some(local_path.to_string_lossy().to_string()),
                failed_files: None,
            },
        );

        Ok(())
    }

    /// Resume a paused or failed batch using fresh Graph content URLs.
    ///
    /// Failed batches resume from the persisted per-chunk progress, so only
    /// the missing byte ranges are re-downloaded.
    pub async fn resume_batch(
        &mut self,
        batch_id: &str,
        base_url: &str,
        token: String,
    ) -> Result<(), AppError> {
        let batch = self
            .batches
            .get_mut(batch_id)
            .ok_or_else(|| AppError::Transfer {
                message: "Task not found".to_string(),
                task_id: batch_id.to_string(),
            })?;

        // Failed(_) carries a message payload, so match the variant rather
        // than compare values.
        if batch.status != TaskStatus::Paused
            && !matches!(batch.status, TaskStatus::Failed(_))
        {
            return Err(AppError::Transfer {
                message: "Task is not paused or failed".to_string(),
                task_id: batch_id.to_string(),
            });
        }

        batch.status = TaskStatus::Downloading;
        batch.error = None;
        log::info!("resuming batch {} ({})", batch_id, batch.name);
        let batch_progress = BatchProgress {
            id: batch.id.clone(),
            name: batch.name.clone(),
            total_bytes: batch.total_bytes,
            downloaded_bytes: Arc::clone(&batch.downloaded_bytes),
        };
        let task_ids = batch.task_ids.clone();

        for task_id in task_ids {
            let Some(task) = self.tasks.get_mut(&task_id) else {
                continue;
            };
            if task.status == TaskStatus::Completed {
                continue;
            }

            task.download_url = format!(
                "{}/drives/{}/items/{}/content",
                base_url, task.drive_id, task.item_id
            );
            task.url_obtained_at = Utc::now();
            task.bearer_token = Some(token.clone());
            task.cancel_token = CancellationToken::new();
            task.status = TaskStatus::Downloading;
            task.started_at = Some(Instant::now());

            self.spawn_child_download(&task_id, Some(batch_progress.clone()));
        }

        // Flip the task card back to "downloading" immediately; the spawned
        // workers only emit once bytes start flowing (they may sit queued on
        // the file-level semaphore).
        let (name, total, downloaded, local) = {
            let Some(batch) = self.batches.get(batch_id) else {
                return Ok(());
            };
            (
                batch.name.clone(),
                batch.total_bytes,
                batch.downloaded_bytes.load(Ordering::Relaxed),
                batch.local_path.to_string_lossy().to_string(),
            )
        };
        let _ = self.app_handle.emit(
            "progress-event",
            ProgressEvent {
                task_id: batch_id.to_string(),
                file_name: name,
                status: "downloading".to_string(),
                total_bytes: total,
                transferred_bytes: downloaded,
                speed_bps: 0,
                elapsed_secs: 0.0,
                error: None,
                local_path: Some(local),
                failed_files: None,
            },
        );

        Ok(())
    }

    /// Cancel a batch, aborting child transfers and deleting partial files.
    pub fn cancel_batch(&mut self, batch_id: &str) -> Result<(), AppError> {
        let batch = self.batches.remove(batch_id).ok_or_else(|| {
            AppError::Transfer {
                message: "Task not found".to_string(),
                task_id: batch_id.to_string(),
            }
        })?;

        for task_id in batch.task_ids {
            if let Some(task) = self.tasks.remove(&task_id) {
                task.cancel_token.cancel();
                if task.local_path.exists() {
                    let _ = fs::remove_file(&task.local_path);
                }
                if let Some(part) = &task.download_path {
                    if part.exists() {
                        let _ = fs::remove_file(part);
                    }
                }
                let _ = fs::remove_file(&task.persist_path);
            }
        }

        let batch_path = self.state_dir.join("batches").join(format!("{}.json", batch_id));
        let _ = fs::remove_file(batch_path);

        Ok(())
    }

    /// Remove a batch from the engine. Completed files are kept; partial files are deleted.
    pub fn remove_batch(&mut self, batch_id: &str) -> Result<(), AppError> {
        let batch = self.batches.remove(batch_id).ok_or_else(|| {
            AppError::Transfer {
                message: "Task not found".to_string(),
                task_id: batch_id.to_string(),
            }
        })?;

        for task_id in batch.task_ids {
            if let Some(task) = self.tasks.remove(&task_id) {
                task.cancel_token.cancel();
                if task.status != TaskStatus::Completed && task.local_path.exists() {
                    let _ = fs::remove_file(&task.local_path);
                }
                if task.status != TaskStatus::Completed {
                    if let Some(part) = &task.download_path {
                        if part.exists() {
                            let _ = fs::remove_file(part);
                        }
                    }
                }
                let _ = fs::remove_file(&task.persist_path);
            }
        }

        let batch_path = self.state_dir.join("batches").join(format!("{}.json", batch_id));
        let _ = fs::remove_file(batch_path);

        Ok(())
    }

    /// Return snapshots of all download batches for startup restore.
    pub fn snapshot_batches(&self) -> Vec<BatchSnapshot> {
        self.batches
            .values()
            .map(|batch| {
                let downloaded = batch.downloaded_bytes.load(Ordering::Relaxed);
                BatchSnapshot {
                    id: batch.id.clone(),
                    name: batch.name.clone(),
                    status: task_status_string(&batch.status),
                    total_bytes: batch.total_bytes,
                    downloaded_bytes: downloaded,
                    speed_bps: 0,
                    elapsed_secs: 0.0,
                    error: batch.error.clone(),
                    local_path: batch.local_path.to_string_lossy().to_string(),
                    cloud_env: batch.cloud_env.clone(),
                    drive_id: batch.drive_id.clone(),
                    home_account_id: batch.home_account_id.clone(),
                    failed_files: batch.failed_files.clone(),
                }
            })
            .collect()
    }

    /// Snapshot a batch's files for integrity verification. Returns the
    /// batch's owning account plus one record per file.
    pub fn batch_files_for_verification(
        &self,
        batch_id: &str,
    ) -> Option<(String, Vec<BatchFileRecord>)> {
        let batch = self.batches.get(batch_id)?;
        let records = batch
            .task_ids
            .iter()
            .filter_map(|task_id| {
                let task = self.tasks.get(task_id)?;
                Some(BatchFileRecord {
                    file_name: task.file_name.clone(),
                    local_path: task.local_path.to_string_lossy().to_string(),
                    total_bytes: task.total_bytes,
                    quick_xor_hash: task.quick_xor_hash.clone(),
                    item_id: task.item_id.clone(),
                    drive_id: task.drive_id.clone(),
                    status: task_status_string(&task.status),
                })
            })
            .collect();
        Some((batch.home_account_id.clone(), records))
    }

    /// Remove completed-but-corrupt files from a batch and build fresh
    /// download params for them. Deletes the bad local artifacts so the
    /// re-download can reclaim the original file name. When nothing is left
    /// in the batch, the batch itself is finished off and dropped.
    pub async fn take_completed_files_for_redownload(
        &mut self,
        batch_id: &str,
        file_names: &[String],
        base_url: &str,
        token: &str,
        cloud_env: &str,
    ) -> Vec<DownloadParams> {
        let Some(batch) = self.batches.get(batch_id) else {
            return Vec::new();
        };
        let home_account_id = batch.home_account_id.clone();

        let mut params = Vec::new();
        let mut removed_ids = Vec::new();
        let mut removed_bytes: u64 = 0;
        for task_id in &batch.task_ids {
            let Some(task) = self.tasks.get(task_id) else {
                continue;
            };
            if task.status != TaskStatus::Completed {
                continue;
            }
            if !file_names.iter().any(|name| *name == task.file_name) {
                continue;
            }

            let _ = fs::remove_file(&task.local_path);
            if let Some(write_path) = &task.download_path {
                let _ = fs::remove_file(write_path);
            }
            let _ = fs::remove_file(&task.persist_path);

            removed_bytes += task.downloaded_bytes.load(Ordering::Relaxed);
            params.push(DownloadParams {
                file_name: task.file_name.clone(),
                home_account_id: home_account_id.clone(),
                cloud_env: cloud_env.to_string(),
                drive_id: task.drive_id.clone(),
                item_id: task.item_id.clone(),
                local_path: task.local_path.clone(),
                total_bytes: task.total_bytes,
                download_url: format!(
                    "{}/drives/{}/items/{}/content",
                    base_url, task.drive_id, task.item_id
                ),
                bearer_token: Some(token.to_string()),
                quick_xor_hash: task.quick_xor_hash.clone(),
            });
            removed_ids.push(task_id.clone());
        }

        if removed_ids.is_empty() {
            return Vec::new();
        }
        for id in &removed_ids {
            self.tasks.remove(id);
        }
        let Some(batch) = self.batches.get_mut(batch_id) else {
            return params;
        };
        batch.task_ids.retain(|id| !removed_ids.contains(id));
        batch.total_bytes = batch.total_bytes.saturating_sub(removed_bytes);
        batch.downloaded_bytes.fetch_sub(removed_bytes, Ordering::Relaxed);

        if batch.task_ids.is_empty() {
            // Nothing left to track: retire the batch outright.
            self.batches.remove(batch_id);
            let batch_path = self
                .state_dir
                .join("batches")
                .join(format!("{}.json", batch_id));
            let _ = fs::remove_file(batch_path);
            let _ = self.app_handle.emit(
                "progress-event",
                ProgressEvent {
                    task_id: batch_id.to_string(),
                    file_name: String::new(),
                    status: "completed".to_string(),
                    total_bytes: 0,
                    transferred_bytes: 0,
                    speed_bps: 0,
                    elapsed_secs: 0.0,
                    error: None,
                    local_path: None,
                    failed_files: None,
                },
            );
            return params;
        }

        let snapshot = batch.clone();
        self.save_batch(&snapshot);
        self.update_batch_status(batch_id).await;
        params
    }

    /// Return the account that owns a batch.
    pub fn batch_home_account_id(&self, batch_id: &str) -> Option<String> {
        self.batches
            .get(batch_id)
            .map(|batch| batch.home_account_id.clone())
    }

    /// Persist the current chunk state of a single child task.
    async fn persist_task_now(&self, task_id: &str) {
        let Some(task) = self.tasks.get(task_id) else {
            return;
        };
        persist_task_file(task).await;
    }

    /// Record a child file's real error message on its batch.
    ///
    /// Replaces any previous entry for the same file so a retried failure
    /// shows the latest error instead of appending duplicates.
    fn record_file_failure(&mut self, batch_id: &str, file_name: &str, message: &str) {
        let Some(batch) = self.batches.get_mut(batch_id) else {
            return;
        };
        let entry = FailedFile {
            file_name: file_name.to_string(),
            message: message.to_string(),
        };
        if let Some(existing) = batch
            .failed_files
            .iter_mut()
            .find(|f| f.file_name == file_name)
        {
            *existing = entry;
        } else {
            batch.failed_files.push(entry);
        }
        let snapshot = batch.clone();
        self.save_batch(&snapshot);
    }

    /// Clear a child file's failure entry after it succeeds on a resumed run.
    fn record_file_success(&mut self, batch_id: &str, file_name: &str) {
        let Some(batch) = self.batches.get_mut(batch_id) else {
            return;
        };
        if batch.failed_files.is_empty() {
            return;
        }
        batch.failed_files.retain(|f| f.file_name != file_name);
        let snapshot = batch.clone();
        self.save_batch(&snapshot);
    }

    fn save_batch(&self, batch: &DownloadBatch) {
        let path = self.state_dir.join("batches").join(format!("{}.json", batch.id));
        let persisted = PersistedBatch {
            id: batch.id.clone(),
            name: batch.name.clone(),
            task_ids: batch.task_ids.clone(),
            total_bytes: batch.total_bytes,
            home_account_id: batch.home_account_id.clone(),
            local_path: batch.local_path.to_string_lossy().to_string(),
            cloud_env: batch.cloud_env.clone(),
            drive_id: batch.drive_id.clone(),
            url_obtained_at: batch.url_obtained_at.to_rfc3339(),
            failed_files: batch.failed_files.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&persisted) {
            let _ = fs::write(path, json);
        }
    }

    /// Recompute a batch's status from its child tasks after a child finishes.
    async fn update_batch_status(&mut self, batch_id: &str) {
        let status = self.batches.get(batch_id).map(|batch| {
            compute_batch_status(&self.tasks, &batch.task_ids)
        });
        let downloaded = self.batches.get(batch_id).map(|batch| {
            batch.task_ids.iter().fold(0u64, |sum, id| {
                sum + self
                    .tasks
                    .get(id)
                    .map(|t| t.downloaded_bytes.load(Ordering::Relaxed))
                    .unwrap_or(0)
            })
        });

        let Some((status, downloaded)) = status.zip(downloaded) else {
            return;
        };
        let completed = status == TaskStatus::Completed;

        let snapshot = {
            let Some(batch) = self.batches.get_mut(batch_id) else {
                return;
            };
            batch.downloaded_bytes.store(downloaded, Ordering::Relaxed);
            batch.status = status;
            batch.error = match &batch.status {
                TaskStatus::Failed(message) => Some(message.clone()),
                _ => None,
            };
            batch.clone()
        };

        self.save_batch(&snapshot);
        // Completed batches stay persisted (batch + per-task records) so the
        // task panel keeps them after a restart and files can be re-verified.
        // They leave the panel only via the explicit remove action.
    }
}

fn chunk_downloaded_sum(chunks: &Mutex<Vec<ChunkState>>) -> u64 {
    match chunks.try_lock() {
        Ok(guard) => guard.iter().map(|c| c.downloaded).sum(),
        Err(_) => 0,
    }
}

fn compute_batch_status(
    tasks: &HashMap<String, DownloadTask>,
    task_ids: &[String],
) -> TaskStatus {
    let statuses = task_ids
        .iter()
        .map(|id| {
            tasks
                .get(id)
                .map(|task| task.status.clone())
                .unwrap_or_else(|| TaskStatus::Failed(String::new()))
        })
        .collect::<Vec<_>>();
    batch_status_from_statuses(&statuses)
}

fn batch_status_from_statuses(statuses: &[TaskStatus]) -> TaskStatus {
    if statuses.is_empty() {
        return TaskStatus::Paused;
    }

    let mut all_completed = true;
    let mut any_downloading = false;
    let mut any_failed = false;

    for status in statuses {
        match status {
            TaskStatus::Completed => {}
            TaskStatus::Downloading | TaskStatus::Queued => {
                all_completed = false;
                any_downloading = true;
            }
            TaskStatus::Paused => {
                all_completed = false;
            }
            TaskStatus::Failed(_) => {
                all_completed = false;
                any_failed = true;
            }
        }
    }

    if all_completed {
        TaskStatus::Completed
    } else if any_downloading {
        TaskStatus::Downloading
    } else if any_failed {
        TaskStatus::Failed("Some files failed to download".to_string())
    } else {
        TaskStatus::Paused
    }
}

fn task_status_string(status: &TaskStatus) -> String {
    match status {
        TaskStatus::Queued => "queued".to_string(),
        TaskStatus::Downloading => "downloading".to_string(),
        TaskStatus::Paused => "paused".to_string(),
        TaskStatus::Completed => "completed".to_string(),
        TaskStatus::Failed(_) => "failed".to_string(),
    }
}

fn task_status_from_str(status: &str) -> Option<TaskStatus> {
    match status {
        "queued" => Some(TaskStatus::Queued),
        "downloading" => Some(TaskStatus::Downloading),
        "paused" => Some(TaskStatus::Paused),
        "completed" => Some(TaskStatus::Completed),
        "failed" => Some(TaskStatus::Failed(String::new())),
        _ => None,
    }
}

fn build_persisted_seed(task: &DownloadTask) -> PersistedTask {
    PersistedTask {
        id: task.id.clone(),
        file_name: task.file_name.clone(),
        home_account_id: task.home_account_id.clone(),
        drive_id: task.drive_id.clone(),
        item_id: task.item_id.clone(),
        local_path: task.local_path.to_string_lossy().to_string(),
        total_bytes: task.total_bytes,
        status: task_status_string(&task.status),
        cloud_env: task.cloud_env.clone(),
        url_obtained_at: task.url_obtained_at.to_rfc3339(),
        chunks: Vec::new(),
        batch_id: task.batch_id.clone(),
        etag: task.etag.clone(),
        download_path: task
            .download_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        quick_xor_hash: task.quick_xor_hash.clone(),
    }
}

async fn persist_task_file(task: &DownloadTask) {
    let chunks = {
        let guard = task.chunks.lock().await;
        guard
            .iter()
            .map(|c| PersistedChunk {
                start: c.start,
                end: c.end,
                downloaded: c.downloaded,
            })
            .collect::<Vec<_>>()
    };

    let persisted = PersistedTask {
        id: task.id.clone(),
        file_name: task.file_name.clone(),
        home_account_id: task.home_account_id.clone(),
        drive_id: task.drive_id.clone(),
        item_id: task.item_id.clone(),
        local_path: task.local_path.to_string_lossy().to_string(),
        total_bytes: task.total_bytes,
        status: task_status_string(&task.status),
        cloud_env: task.cloud_env.clone(),
        url_obtained_at: task.url_obtained_at.to_rfc3339(),
        chunks,
        batch_id: task.batch_id.clone(),
        etag: task.etag.clone(),
        download_path: task
            .download_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        quick_xor_hash: task.quick_xor_hash.clone(),
    };

    if let Ok(json) = serde_json::to_string_pretty(&persisted) {
        let _ = fs::write(&task.persist_path, json);
    }
}

fn restore_task(persisted: PersistedTask, tasks_dir: &Path) -> DownloadTask {
    let status = match persisted.status.as_str() {
        "completed" => TaskStatus::Completed,
        "failed" => TaskStatus::Failed(String::new()),
        _ => TaskStatus::Paused,
    };
    let chunks = Arc::new(Mutex::new(
        persisted
            .chunks
            .into_iter()
            .map(|c| ChunkState {
                start: c.start,
                end: c.end,
                downloaded: c.downloaded,
            })
            .collect::<Vec<_>>(),
    ));
    let downloaded = match chunks.try_lock() {
        Ok(guard) => guard.iter().map(|c| c.downloaded).sum(),
        Err(_) => 0,
    };
    let id = persisted.id.clone();
    let url_obtained_at = DateTime::parse_from_rfc3339(&persisted.url_obtained_at)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now() - Duration::hours(2));
    let local_path = PathBuf::from(persisted.local_path);
    let download_path = match persisted.download_path {
        Some(p) => {
            // Tasks recorded with the old `.part` name upgrade to the
            // sync-invisible `.tmp` name on restore.
            let p = PathBuf::from(p);
            let legacy_part = {
                let mut os = local_path.as_os_str().to_os_string();
                os.push(".part");
                PathBuf::from(os)
            };
            if p == legacy_part {
                migrate_partial_to_part(&local_path).or(Some(p))
            } else {
                Some(p)
            }
        }
        // Legacy task without a recorded .part path: try to move the partial
        // file aside so the final name stays free until completion.
        None if status != TaskStatus::Completed => {
            migrate_partial_to_part(&local_path)
        }
        None => None,
    };

    DownloadTask {
        id: id.clone(),
        file_name: persisted.file_name,
        home_account_id: persisted.home_account_id,
        drive_id: persisted.drive_id,
        item_id: persisted.item_id,
        local_path,
        total_bytes: persisted.total_bytes,
        downloaded_bytes: Arc::new(AtomicU64::new(downloaded)),
        status,
        cloud_env: persisted.cloud_env,
        download_url: String::new(),
        bearer_token: None,
        url_obtained_at,
        chunks,
        cancel_token: CancellationToken::new(),
        started_at: None,
        batch_id: persisted.batch_id,
        etag: persisted.etag,
        persist_path: tasks_dir.join(format!("{}.json", id)),
        download_path,
        quick_xor_hash: persisted.quick_xor_hash,
    }
}

/// Try to move a legacy partial download to `<name>.part`.
///
/// Returns None when there is nothing to move or the file is locked (e.g. by
/// the sync client), in which case the download keeps writing in place.
fn migrate_partial_to_part(local_path: &Path) -> Option<PathBuf> {
    let tmp = derive_write_path(local_path);
    if tmp.exists() {
        return Some(tmp);
    }
    // Upgrade the legacy `.part` name to `.tmp` so the sync client stops
    // picking the partial up. If the rename is blocked (file held by the
    // sync client right now), keep resuming the `.part` as-is.
    let legacy = {
        let mut os = local_path.as_os_str().to_os_string();
        os.push(".part");
        PathBuf::from(os)
    };
    if legacy.exists() {
        return match fs::rename(&legacy, &tmp) {
            Ok(()) => Some(tmp),
            Err(_) => Some(legacy),
        };
    }
    if !local_path.exists() {
        return None;
    }
    match fs::rename(local_path, &tmp) {
        Ok(()) => Some(tmp),
        Err(_) => None,
    }
}

fn load_state(state_dir: &Path) -> (Vec<PersistedTask>, Vec<PersistedBatch>) {
    let mut tasks = Vec::new();
    let mut batches = Vec::new();

    let tasks_dir = state_dir.join("tasks");
    if let Ok(entries) = fs::read_dir(&tasks_dir) {
        for entry in entries.flatten() {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                if let Ok(task) = serde_json::from_str::<PersistedTask>(&content) {
                    tasks.push(task);
                }
            }
        }
    }

    let batches_dir = state_dir.join("batches");
    if let Ok(entries) = fs::read_dir(&batches_dir) {
        for entry in entries.flatten() {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                if let Ok(batch) = serde_json::from_str::<PersistedBatch>(&content) {
                    batches.push(batch);
                }
            }
        }
    }

    (tasks, batches)
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

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
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

/// Run the actual download, downloading all chunks in parallel.
///
/// Bytes are written to `write_path` (normally `<name>.part`); once every
/// chunk is complete the file is renamed onto `final_path`.
async fn run_download(
    http_client: Client,
    app_handle: AppHandle,
    batch_progress: Option<BatchProgress>,
    task_id: String,
    file_name: String,
    download_url: String,
    bearer_token: Option<String>,
    write_path: PathBuf,
    final_path: PathBuf,
    quick_xor_hash: Option<String>,
    total_bytes: u64,
    chunks: Arc<Mutex<Vec<ChunkState>>>,
    downloaded_bytes: Arc<AtomicU64>,
    cancel_token: CancellationToken,
    persist_path: PathBuf,
    seed: PersistedTask,
) -> Result<(), String> {
    if total_bytes == 0 {
        let _ = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&write_path)
            .await
            .map_err(|e| format!("Failed to create empty file: {}", e))?;
        return finalize_download(write_path, final_path).await;
    }

    // Create or open the output file
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&write_path)
        .await
        .map_err(|e| format!("Failed to open output file: {}", e))?;

    // Pre-allocate the file to the full size
    file.set_len(total_bytes)
        .await
        .map_err(|e| format!("Failed to set file size: {}", e))?;

    drop(file);

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CHUNKS));
    let mut handles = Vec::new();

    let emit_id = batch_progress
        .as_ref()
        .map(|b| b.id.clone())
        .unwrap_or_else(|| task_id.clone());
    let emit_name = batch_progress
        .as_ref()
        .map(|b| b.name.clone())
        .unwrap_or_else(|| file_name.clone());
    let emit_total = batch_progress
        .as_ref()
        .map(|b| b.total_bytes)
        .unwrap_or(total_bytes);
    let emit_downloaded = batch_progress
        .as_ref()
        .map(|b| Arc::clone(&b.downloaded_bytes))
        .unwrap_or_else(|| Arc::clone(&downloaded_bytes));
    let batch_downloaded = batch_progress
        .as_ref()
        .map(|b| Arc::clone(&b.downloaded_bytes));

    // Progress tracking for speed calculation
    let progress_cancel = cancel_token.clone();
    let progress_downloaded = emit_downloaded;
    let progress_app_handle = app_handle.clone();
    let progress_task_id = emit_id;
    let progress_file_name = emit_name;
    let progress_total = emit_total;
    let progress_chunks = Arc::clone(&chunks);
    let progress_persist_path = persist_path.clone();
    let progress_seed = seed.clone();

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

            let status = if current_bytes >= progress_total {
                "completed"
            } else {
                "downloading"
            };

            // Persist exact chunk state for crash/restart recovery.
            let mut persisted = progress_seed.clone();
            persisted.status = status.to_string();
            persisted.chunks = {
                let guard = progress_chunks.lock().await;
                guard
                    .iter()
                    .map(|c| PersistedChunk {
                        start: c.start,
                        end: c.end,
                        downloaded: c.downloaded,
                    })
                    .collect()
            };
            if let Ok(json) = serde_json::to_string_pretty(&persisted) {
                let _ = fs::write(&progress_persist_path, json);
            }

            let _ = progress_app_handle.emit(
                "progress-event",
                ProgressEvent {
                    task_id: progress_task_id.clone(),
                    file_name: progress_file_name.clone(),
                    status: status.to_string(),
                    total_bytes: progress_total,
                    transferred_bytes: current_bytes,
                    speed_bps: speed,
                    elapsed_secs: elapsed,
                    error: None,
                    local_path: None,
                    failed_files: None,
                },
            );

            if current_bytes >= progress_total {
                break;
            }
        }
    });

    // Spawn chunk download tasks
    let chunk_snapshot = {
        let guard = chunks.lock().await;
        guard.clone()
    };
    for (chunk_index, chunk) in chunk_snapshot.iter().enumerate() {
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
        let path = write_path.clone();
        let token = cancel_token.clone();
        let bearer_token = bearer_token.clone();
        let bytes_counter = Arc::clone(&downloaded_bytes);
        let chunk_counter = Arc::clone(&chunks);
        let batch_counter = batch_downloaded.clone();

        let chunk_start = chunk.start;
        let end = chunk.end;

        let handle = tokio::spawn(async move {
            let _permit = permit;
            download_chunk(
                client,
                url,
                path,
                chunk_start,
                end,
                chunk_index,
                chunk_counter,
                bytes_counter,
                batch_counter,
                token,
                bearer_token,
            )
            .await
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
        return Err(err);
    }

    if let Err(e) = verify_download_integrity(&write_path, total_bytes, quick_xor_hash.as_deref()).await {
        // The bytes on disk are not what the cloud says they should be; scrap
        // the recorded progress so the next resume re-downloads from zero.
        reset_download_progress(&chunks, &downloaded_bytes, batch_downloaded.as_deref()).await;
        log::warn!("integrity check failed for task {} ({}): {}", task_id, file_name, e);
        return Err(e);
    }

    finalize_download(write_path, final_path).await
}

/// Verify a fully downloaded file: size must match the cloud item, and when
/// the cloud provided a quickXorHash the whole content is re-hashed and
/// compared. Called once after every chunk has completed.
pub(crate) async fn verify_download_integrity(
    path: &Path,
    total_bytes: u64,
    quick_xor_hash: Option<&str>,
) -> Result<(), String> {
    let meta = tokio::fs::metadata(path)
        .await
        .map_err(|e| format!("Failed to stat downloaded file: {}", e))?;
    if meta.len() != total_bytes {
        return Err(format!(
            "Downloaded file size mismatch: expected {} bytes, got {}",
            total_bytes,
            meta.len()
        ));
    }

    let Some(expected) = quick_xor_hash else {
        return Ok(());
    };

    let mut hasher = quickxorhash::QuickXorHash::new();
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("Failed to open downloaded file for hashing: {}", e))?;
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .await
            .map_err(|e| format!("Failed to read downloaded file for hashing: {}", e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    use base64::Engine as _;
    let actual = base64::engine::general_purpose::STANDARD.encode(hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!(
            "Content hash mismatch (quickXorHash): expected {}, got {} — progress was reset, resume to re-download",
            expected, actual
        ));
    }
    Ok(())
}

/// Zero out recorded progress after a failed integrity check so the next
/// resume re-downloads every chunk instead of trusting corrupt bytes.
async fn reset_download_progress(
    chunks: &Mutex<Vec<ChunkState>>,
    downloaded_bytes: &AtomicU64,
    batch_downloaded: Option<&AtomicU64>,
) {
    let old = downloaded_bytes.swap(0, Ordering::Relaxed);
    if let Some(batch) = batch_downloaded {
        batch.fetch_sub(old, Ordering::Relaxed);
    }
    let mut guard = chunks.lock().await;
    for chunk in guard.iter_mut() {
        chunk.downloaded = 0;
    }
}

/// Move a completed download onto its final file name.
///
/// Retries briefly because sync clients (OneDrive) may hold the `.part` file
/// right after the last bytes land, and falls back to a conflict-resolved
/// name instead of overwriting an existing file. A task whose chunks are all
/// complete but that failed here finishes on the next resume: the chunk loop
/// is then a no-op and only the rename is retried.
async fn finalize_download(write_path: PathBuf, final_path: PathBuf) -> Result<(), String> {
    if write_path == final_path {
        return Ok(());
    }

    let retry_delays_ms = [300, 900, 2000];
    let mut last_err = String::new();
    for attempt in 0..=retry_delays_ms.len() {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(
                retry_delays_ms[attempt - 1],
            ))
            .await;
        }
        let destination = if final_path.exists() {
            resolve_conflict(&final_path)
        } else {
            final_path.clone()
        };
        match tokio::fs::rename(&write_path, &destination).await {
            Ok(()) => {
                log::info!(
                    "finalized download: {} -> {}",
                    write_path.display(),
                    destination.display()
                );
                return Ok(());
            }
            Err(e) => {
                last_err = format!(
                    "Failed to finalize download (rename {} -> {}): {}",
                    write_path.display(),
                    destination.display(),
                    e
                );
                log::warn!("finalize attempt {} failed: {}", attempt + 1, last_err);
            }
        }
    }
    Err(last_err)
}

/// Retry attempts per chunk before the whole file task fails. Progress is
/// persisted after every attempt, so a later resume only fetches what is
/// still missing.
const CHUNK_RETRY_ATTEMPTS: usize = 3;

/// Whether a `Content-Range: bytes <start>-<end>/<total>` header value claims
/// to begin at `start`. Unparseable headers pass (nothing to validate against).
fn content_range_starts_at(content_range: &str, start: u64) -> bool {
    let rest = match content_range.trim().split_once(' ') {
        Some((unit, rest)) if unit.eq_ignore_ascii_case("bytes") => rest.trim(),
        _ => return true,
    };
    match rest.split('-').next() {
        Some(first) => first.trim().parse::<u64>().map(|s| s == start).unwrap_or(true),
        None => true,
    }
}

/// Download a single chunk with per-chunk retry and resume-from-offset.
///
/// `start`/`end` are the chunk's absolute bounds in the file; each attempt
/// resumes from the chunk's recorded progress. A connection that dies before
/// the full range arrives is retried like any other transient error instead
/// of being accepted as a complete chunk (which would leave zero-filled holes
/// in a file marked "completed").
async fn download_chunk(
    client: Client,
    url: String,
    path: PathBuf,
    start: u64,
    end: u64,
    chunk_index: usize,
    chunks: Arc<Mutex<Vec<ChunkState>>>,
    downloaded_bytes: Arc<AtomicU64>,
    batch_downloaded: Option<Arc<AtomicU64>>,
    cancel_token: CancellationToken,
    bearer_token: Option<String>,
) -> Result<(), String> {
    let expected = end - start + 1;
    let mut last_err = String::from("download did not start");

    for attempt in 0..CHUNK_RETRY_ATTEMPTS {
        if cancel_token.is_cancelled() {
            return Err("Download cancelled".to_string());
        }

        let done = {
            let guard = chunks.lock().await;
            guard
                .get(chunk_index)
                .map(|c| c.downloaded)
                .unwrap_or(expected)
        };
        if done >= expected {
            return Ok(());
        }

        match fetch_chunk_range(
            client.clone(),
            &url,
            &path,
            start + done,
            end,
            chunk_index,
            &chunks,
            &downloaded_bytes,
            batch_downloaded.as_ref(),
            &cancel_token,
            bearer_token.as_deref(),
        )
        .await
        {
            Ok(()) => {
                let done = {
                    let guard = chunks.lock().await;
                    guard
                        .get(chunk_index)
                        .map(|c| c.downloaded)
                        .unwrap_or(expected)
                };
                if done >= expected {
                    return Ok(());
                }
                // The server closed the stream early; whatever landed on disk
                // is kept and the remaining range is fetched by the next try.
                last_err = format!(
                    "connection closed early: {} of {} bytes received",
                    done, expected
                );
            }
            Err(e) => {
                if e.contains("cancelled") {
                    return Err(e);
                }
                last_err = e;
                if !is_retryable_chunk_error(&last_err) {
                    return Err(last_err);
                }
            }
        }

        let delay = std::time::Duration::from_millis(1000 << attempt);
        tokio::select! {
            _ = cancel_token.cancelled() => {
                return Err("Download cancelled".to_string());
            }
            _ = tokio::time::sleep(delay) => {}
        }
    }

    Err(format!(
        "chunk {} failed after {} attempts: {}",
        chunk_index, CHUNK_RETRY_ATTEMPTS, last_err
    ))
}

/// Whether a chunk failure is worth retrying. Client errors other than 429
/// (auth, missing file, forbidden) and local filesystem failures are
/// permanent; resets, stream errors, throttling and 5xx are transient.
fn is_retryable_chunk_error(err: &str) -> bool {
    if err.contains("cancelled") {
        return false;
    }
    // A sharing violation (os error 32) means a sync client or scanner held
    // the temp file at open time; it lets go shortly after, so retry.
    if err.contains("os error 32") {
        return true;
    }
    const PERMANENT_STATUS: &[u16] = &[200, 400, 401, 403, 404, 405, 409, 410, 412, 416];
    for code in PERMANENT_STATUS {
        if err.contains(&format!("Unexpected status code: {}", code)) {
            return false;
        }
    }
    !(err.contains("Write error")
        || err.contains("Failed to open file")
        || err.contains("Failed to seek"))
}

/// One uninterrupted range fetch for a chunk: request `bytes={start}-{end}`,
/// stream the body to the file at `start`, and update the chunk counters.
/// Returns Ok even when the stream ends before `end` — the caller compares
/// the recorded progress against the chunk size and retries the remainder.
#[allow(clippy::too_many_arguments)]
async fn fetch_chunk_range(
    client: Client,
    url: &str,
    path: &Path,
    start: u64,
    end: u64,
    chunk_index: usize,
    chunks: &Arc<Mutex<Vec<ChunkState>>>,
    downloaded_bytes: &Arc<AtomicU64>,
    batch_downloaded: Option<&Arc<AtomicU64>>,
    cancel_token: &CancellationToken,
    bearer_token: Option<&str>,
) -> Result<(), String> {
    let range = format!("bytes={}-{}", start, end);

    let mut request = client.get(url).header("Range", &range);
    if let Some(token) = bearer_token {
        request = request.bearer_auth(token);
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status = response.status().as_u16();
    // 200 means the server ignored the Range header and returned the whole
    // file; only safe when the chunk starts at offset 0 (bytes are capped
    // below). For any other offset it would corrupt the file.
    if status != 206 && !(status == 200 && start == 0) {
        return Err(format!("Unexpected status code: {}", status));
    }
    if status == 206 {
        if let Some(cr) = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
        {
            if !content_range_starts_at(cr, start) {
                // A 206 that does not cover the requested offset would write
                // the wrong bytes at `start`; treat it as a bad response.
                return Err(format!(
                    "Content-Range mismatch: got {:?}, requested start {}",
                    cr, start
                ));
            }
        }
    }

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .await
        .map_err(|e| format!("Failed to open file for writing: {}", e))?;

    file.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(|e| format!("Failed to seek: {}", e))?;

    let expected = end - start + 1;
    let mut written: u64 = 0;
    let mut stream = response.bytes_stream();

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                return Err("Download cancelled".to_string());
            }
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(bytes)) => {
                        let take = std::cmp::min(bytes.len() as u64, expected - written);
                        if take < bytes.len() as u64 {
                            // Response carries more than this chunk's range
                            // (Range ignored): consume only the chunk's part.
                            file.write_all(&bytes[..take as usize]).await
                                .map_err(|e| format!("Write error: {}", e))?;
                        } else {
                            file.write_all(&bytes).await
                                .map_err(|e| format!("Write error: {}", e))?;
                        }
                        written += take;
                        {
                            let mut guard = chunks.lock().await;
                            if let Some(chunk) = guard.get_mut(chunk_index) {
                                chunk.downloaded = chunk.downloaded.saturating_add(take);
                            }
                        }
                        downloaded_bytes.fetch_add(take, Ordering::Relaxed);
                        if let Some(batch) = batch_downloaded {
                            batch.fetch_add(take, Ordering::Relaxed);
                        }
                        if written >= expected {
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        return Err(format!("Stream error: {}", e));
                    }
                    None => {
                        // Stream complete (or prematurely closed); the caller
                        // verifies the byte count against the chunk size.
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
    #[serde(rename = "remoteItem")]
    remote_item: Option<FolderRemoteItem>,
    package: Option<serde_json::Value>,
    file: Option<ChildFileMeta>,
    #[serde(rename = "@microsoft.graph.downloadUrl")]
    download_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChildFileMeta {
    hashes: Option<ChildHashes>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChildHashes {
    quick_xor_hash: Option<String>,
}

impl FolderChildItem {
    fn quick_xor_hash(&self) -> Option<String> {
        if let Some(h) = self.file.as_ref().and_then(|f| f.hashes.as_ref()) {
            return h.quick_xor_hash.clone();
        }
        self.remote_item
            .as_ref()
            .and_then(|r| r.file.as_ref())
            .and_then(|f| f.hashes.as_ref())
            .and_then(|h| h.quick_xor_hash.clone())
    }
}

#[derive(Debug, Deserialize)]
struct FolderRemoteItem {
    folder: Option<serde_json::Value>,
    package: Option<serde_json::Value>,
    file: Option<ChildFileMeta>,
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
    cloud_env: &str,
    home_account_id: &str,
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
            let quick_xor_hash = item.quick_xor_hash();
            let item_name = item.name.unwrap_or_default();
            if item_name.is_empty() {
                continue;
            }

            let child_path = local_base_path.join(&item_name);

            if item.folder.is_some()
                || item.package.is_some()
                || item
                    .remote_item
                    .as_ref()
                    .and_then(|r| r.folder.as_ref().or(r.package.as_ref()))
                    .is_some()
            {
                // It's a subfolder — recurse into it
                let item_id = item.id.unwrap_or_default();
                if item_id.is_empty() {
                    continue;
                }

                let sub_params = Box::pin(download_folder_recursive(
                    http_client,
                    base_url,
                    token,
                    cloud_env,
                    home_account_id,
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
                    home_account_id: home_account_id.to_string(),
                    cloud_env: cloud_env.to_string(),
                    drive_id: drive_id.to_string(),
                    item_id,
                    local_path: child_path,
                    total_bytes: item.size.unwrap_or(0),
                    download_url,
                    bearer_token: Some(token.to_string()),
                    quick_xor_hash,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_cover_the_entire_file() {
        let total = 10_000_000u64;
        let chunks = calculate_chunks(total);
        assert_eq!(chunks.len(), 8);

        let covered: u64 = chunks.iter().map(|c| c.end - c.start + 1).sum();
        assert_eq!(covered, total);
        assert!(chunks.iter().all(|c| c.end >= c.start));
    }

    #[test]
    fn interrupted_task_restores_as_paused_with_exact_progress() {
        let persisted = PersistedTask {
            id: "task-1".to_string(),
            file_name: "file.bin".to_string(),
            home_account_id: "account-1".to_string(),
            drive_id: "drive".to_string(),
            item_id: "item".to_string(),
            local_path: "C:\\downloads\\file.bin".to_string(),
            total_bytes: 1024,
            status: "downloading".to_string(),
            cloud_env: "global".to_string(),
            url_obtained_at: "2026-08-14T00:00:00Z".to_string(),
            chunks: vec![PersistedChunk {
                start: 0,
                end: 1023,
                downloaded: 512,
            }],
            batch_id: "batch-1".to_string(),
            etag: None,
            download_path: None,
            quick_xor_hash: None,
        };

        let task = restore_task(persisted, Path::new("."));
        assert_eq!(task.status, TaskStatus::Paused);
        assert_eq!(
            task.downloaded_bytes.load(Ordering::Relaxed),
            512
        );
        assert_eq!(task.batch_id, "batch-1");
    }

    #[test]
    fn batch_status_stays_downloading_until_all_children_finish() {
        assert_eq!(
            batch_status_from_statuses(&[
                TaskStatus::Completed,
                TaskStatus::Downloading,
                TaskStatus::Paused,
            ]),
            TaskStatus::Downloading
        );
        assert_eq!(
            batch_status_from_statuses(&[
                TaskStatus::Completed,
                TaskStatus::Completed,
            ]),
            TaskStatus::Completed
        );
    }

    #[test]
    fn transient_chunk_errors_are_retried_and_permanent_ones_fail_fast() {
        // Network resets, throttling and 5xx are transient.
        assert!(is_retryable_chunk_error(
            "HTTP request failed: connection reset by peer"
        ));
        assert!(is_retryable_chunk_error(
            "Stream error: connection closed before message completed"
        ));
        assert!(is_retryable_chunk_error("Unexpected status code: 429"));
        assert!(is_retryable_chunk_error("Unexpected status code: 503"));
        // A server that closes the stream early leaves a short chunk behind;
        // the remaining range must be retried rather than accepted as done.
        assert!(is_retryable_chunk_error(
            "connection closed early: 100 of 1000 bytes received"
        ));

        // Cancellation and permanent errors must fail the task immediately.
        assert!(!is_retryable_chunk_error("Download cancelled"));
        assert!(!is_retryable_chunk_error("Unexpected status code: 401"));
        assert!(!is_retryable_chunk_error("Unexpected status code: 404"));
        assert!(!is_retryable_chunk_error("Write error: Access is denied"));
        // A 200 to a non-zero offset means the server ignored the Range
        // header; retrying would corrupt the file.
        assert!(!is_retryable_chunk_error("Unexpected status code: 200"));
    }

    #[test]
    fn legacy_partial_migrates_to_tmp_file() {
        let dir = std::env::temp_dir().join(format!("sol-migrate-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp dir");
        let partial = dir.join("setup.exe");
        fs::write(&partial, b"partial-data").expect("write partial");

        let migrated = migrate_partial_to_part(&partial).expect("migration succeeds");
        assert_eq!(migrated, dir.join("setup.exe.tmp"));
        assert!(!partial.exists());
        assert!(migrated.exists());

        // A second migration finds the .tmp already in place.
        let again = migrate_partial_to_part(&partial).expect("idempotent migration");
        assert_eq!(again, migrated);

        // A missing partial cannot be migrated.
        let missing = dir.join("gone.bin");
        assert!(migrate_partial_to_part(&missing).is_none());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn legacy_part_files_upgrade_to_tmp_names() {
        let dir = std::env::temp_dir().join(format!("sol-part-upgrade-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp dir");
        let partial = dir.join("setup.exe");
        let legacy_part = dir.join("setup.exe.part");
        fs::write(&legacy_part, b"partial-data").expect("write legacy part");

        let migrated = migrate_partial_to_part(&partial).expect("upgrade succeeds");
        assert_eq!(migrated, dir.join("setup.exe.tmp"));
        assert!(!legacy_part.exists());

        // When both names exist (interrupted upgrade), the .tmp name wins:
        // it is what current runs write to, and renames are atomic so the
        // .tmp copy is never a half-written leftover.
        let both = dir.join("both.iso");
        fs::write(dir.join("both.iso.part"), b"old").expect("write part");
        fs::write(dir.join("both.iso.tmp"), b"new").expect("write tmp");
        let kept = migrate_partial_to_part(&both).expect("both names stay resumable");
        assert_eq!(kept, dir.join("both.iso.tmp"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn temp_names_avoid_the_sync_client() {
        // Regular names get a .tmp suffix, which OneDrive never syncs.
        assert_eq!(
            derive_write_path(Path::new("D:/dl/setup.exe")),
            PathBuf::from("D:/dl/setup.exe.tmp")
        );
        // A final name that is itself .tmp falls back to the .part suffix.
        assert_eq!(
            derive_write_path(Path::new("D:/dl/archive.tmp")),
            PathBuf::from("D:/dl/archive.tmp.part")
        );
    }

    #[test]
    fn sharing_violations_are_retried_but_access_denied_is_not() {
        assert!(is_retryable_chunk_error(
            "Failed to open file for writing: sharing violation (os error 32)"
        ));
        assert!(!is_retryable_chunk_error(
            "Failed to open file for writing: access denied (os error 5)"
        ));
    }

    #[tokio::test]
    async fn integrity_check_accepts_correct_hash_and_rejects_wrong_one() {
        let dir = std::env::temp_dir().join(format!("sol-integrity-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp dir");
        let file = dir.join("data.bin");

        // "hello world" quickXorHash — the vector from the quickxorhash crate
        // docs (matching the Microsoft reference implementation).
        fs::write(&file, b"hello world").expect("write file");
        let digest: [u8; 20] = [
            104, 40, 3, 27, 216, 240, 6, 16, 220, 225, 13, 114, 107, 3, 25, 0, 0, 0, 0, 0,
        ];
        use base64::Engine as _;
        let expected = base64::engine::general_purpose::STANDARD.encode(digest);

        verify_download_integrity(&file, 11, Some(&expected))
            .await
            .expect("matching content passes");

        verify_download_integrity(&file, 11, None)
            .await
            .expect("no hash means size-only check");

        let err = verify_download_integrity(&file, 11, Some("AAAAAAAAAAAAAAAAAAAAAAAAAAA="))
            .await
            .expect_err("wrong hash fails");
        assert!(err.contains("hash mismatch"), "{}", err);

        let err = verify_download_integrity(&file, 999, None)
            .await
            .expect_err("wrong size fails");
        assert!(err.contains("size mismatch"), "{}", err);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn content_range_header_is_validated_against_the_requested_start() {
        assert!(content_range_starts_at("bytes 100-199/500", 100));
        assert!(content_range_starts_at("BYTES 100-199/500", 100));
        assert!(!content_range_starts_at("bytes 200-299/500", 100));
        assert!(content_range_starts_at("garbage", 100));
    }

    #[test]
    fn a_200_that_ignored_the_range_header_is_a_permanent_error() {
        assert!(!is_retryable_chunk_error(
            "Unexpected status code: 200 (server ignored Range header)"
        ));
    }
}
