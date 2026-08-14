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
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::errors::AppError;
use crate::models::ProgressEvent;

/// Maximum concurrency for parallel chunk downloads.
const MAX_CONCURRENT_CHUNKS: usize = 8;

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

        tokio::spawn(async move {
            let result = run_download(
                http_client,
                app_handle.clone(),
                batch_progress.clone(),
                task_id.clone(),
                file_name.clone(),
                download_url,
                bearer_token,
                local_path.clone(),
                total_bytes,
                chunks,
                downloaded_bytes,
                cancel_token,
                persist_path,
                seed,
            )
            .await;

            let local_path_display = local_path.to_string_lossy().to_string();
            let (status, error) = match &result {
                Ok(()) => ("completed".to_string(), None),
                Err(e) if e.contains("cancelled") => ("paused".to_string(), None),
                Err(e) => ("failed".to_string(), Some(e.clone())),
            };

            let mut emitted_status = status.clone();
            let mut emit_error = error;
            let mut task_exists = true;
            if let Some(engine_state) = app_handle.try_state::<Mutex<DownloadEngine>>() {
                let mut engine = engine_state.lock().await;
                task_exists = engine.tasks.contains_key(&task_id);
                if let Some(task) = engine.tasks.get_mut(&task_id) {
                    task.status = task_status_from_str(&status).unwrap_or(TaskStatus::Failed(String::new()));
                    engine.persist_task_now(&task_id).await;
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
            },
        );

        Ok(())
    }

    /// Resume a paused batch using fresh Graph content URLs.
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

        if batch.status != TaskStatus::Paused {
            return Err(AppError::Transfer {
                message: "Task is not paused".to_string(),
                task_id: batch_id.to_string(),
            });
        }

        batch.status = TaskStatus::Downloading;
        batch.error = None;
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
                }
            })
            .collect()
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

        if completed {
            for task_id in &snapshot.task_ids {
                if let Some(task) = self.tasks.get(task_id) {
                    let _ = fs::remove_file(&task.persist_path);
                }
            }
            let batch_path = self
                .state_dir
                .join("batches")
                .join(format!("{}.json", batch_id));
            let _ = fs::remove_file(batch_path);
        }
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

    DownloadTask {
        id: id.clone(),
        file_name: persisted.file_name,
        home_account_id: persisted.home_account_id,
        drive_id: persisted.drive_id,
        item_id: persisted.item_id,
        local_path: PathBuf::from(persisted.local_path),
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
async fn run_download(
    http_client: Client,
    app_handle: AppHandle,
    batch_progress: Option<BatchProgress>,
    task_id: String,
    file_name: String,
    download_url: String,
    bearer_token: Option<String>,
    local_path: PathBuf,
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
            .open(&local_path)
            .await
            .map_err(|e| format!("Failed to create empty file: {}", e))?;
        return Ok(());
    }

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
        let path = local_path.clone();
        let token = cancel_token.clone();
        let bearer_token = bearer_token.clone();
        let bytes_counter = Arc::clone(&downloaded_bytes);
        let chunk_counter = Arc::clone(&chunks);
        let batch_counter = batch_downloaded.clone();

        let actual_start = chunk.start + chunk.downloaded;
        let end = chunk.end;

        let handle = tokio::spawn(async move {
            let _permit = permit;
            download_chunk(
                client,
                url,
                path,
                actual_start,
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

    Ok(())
}

/// Download a single chunk using Range headers.
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
    let range = format!("bytes={}-{}", start, end);

    let mut request = client.get(&url).header("Range", &range);
    if let Some(token) = bearer_token {
        request = request.bearer_auth(token);
    }

    let response = request
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
                        let len = bytes.len() as u64;
                        {
                            let mut guard = chunks.lock().await;
                            if let Some(chunk) = guard.get_mut(chunk_index) {
                                chunk.downloaded = chunk.downloaded.saturating_add(len);
                            }
                        }
                        downloaded_bytes.fetch_add(len, Ordering::Relaxed);
                        if let Some(batch) = &batch_downloaded {
                            batch.fetch_add(len, Ordering::Relaxed);
                        }
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
    #[serde(rename = "remoteItem")]
    remote_item: Option<FolderRemoteItem>,
    package: Option<serde_json::Value>,
    #[serde(rename = "@microsoft.graph.downloadUrl")]
    download_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FolderRemoteItem {
    folder: Option<serde_json::Value>,
    package: Option<serde_json::Value>,
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
}
