// Cloud auto backup: uploads the config backup payload to a user-chosen
// folder in a signed-in account's OneDrive via Graph API. Replaces the old
// local-directory design (users no longer need a locally-synced OneDrive).

use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use crate::auth::cloud_config::CloudEnvironment;
use crate::auth::AuthModule;
use crate::errors::AppError;
use crate::graph::GraphClient;
use crate::models::AutoBackupTarget;

use super::backup_snapshot;

/// What caused this backup upload. Startup/Exit triggers only fire when the
/// dynamic sections (chat history / catalog) are selected AND changed since
/// the last upload; Change triggers always upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupTrigger {
    /// App launch, after sessions are restored.
    Startup,
    /// App exit — captures chat/download changes that never touched config.
    Exit,
    /// A config or account save (settings change, login, import, …).
    Change,
}

impl BackupTrigger {
    /// Startup/Exit are gated on dynamic content actually having changed.
    fn gated_on_dynamic_change(&self) -> bool {
        matches!(self, BackupTrigger::Startup | BackupTrigger::Exit)
    }
}

/// File name written into the target OneDrive folder.
pub const AUTO_BACKUP_FILE_NAME: &str = "ShareOneList-backup.json";

/// How long to wait when the target account's session is not restored yet
/// (the startup backup can fire right after launch, racing session restore).
const SESSION_WAIT: Duration = Duration::from_secs(3);

/// Simple upload (one PUT) is used up to this size; larger payloads switch
/// to a resumable upload session. 3.5 MiB leaves headroom under Graph's
/// 4 MiB simple-upload limit.
const SIMPLE_UPLOAD_LIMIT: usize = 3 * 1024 * 1024 + 512 * 1024;

/// Chunk size for resumable-session uploads.
const SESSION_CHUNK_SIZE: usize = 4 * 1024 * 1024;

/// How long the exit-time backup may block app shutdown.
const BLOCKING_BACKUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Build the Graph simple-upload URL for the backup file inside `folder_id`.
///
/// `conflictBehavior` goes in the query string: that is where Graph reads it
/// for simple uploads, and `@` is not a legal HTTP header character anyway
/// (reqwest refuses to even send a request carrying such a header).
fn backup_upload_url(base_url: &str, drive_id: &str, folder_id: &str) -> String {
    format!(
        "{}/drives/{}/items/{}:/{}:/content?@microsoft.graph.conflictBehavior=replace",
        base_url, drive_id, folder_id, AUTO_BACKUP_FILE_NAME
    )
}

/// Build the createUploadSession URL for payloads above the simple limit.
fn backup_session_url(base_url: &str, drive_id: &str, folder_id: &str) -> String {
    format!(
        "{}/drives/{}/items/{}:/{}:/createUploadSession",
        base_url, drive_id, folder_id, AUTO_BACKUP_FILE_NAME
    )
}

/// One upload attempt's PUT request. Kept separate so a test can prove it
/// builds — an invalid header fails silently inside reqwest's builder and
/// only surfaces as a send error.
fn build_put(
    http: &reqwest::Client,
    url: &str,
    token: &str,
    body: Vec<u8>,
) -> reqwest::RequestBuilder {
    http.put(url)
        .bearer_auth(token)
        .header("Content-Type", "application/json")
        .body(body)
}

/// Upload the backup payload as `ShareOneList-backup.json` into the target
/// folder. `conflictBehavior=replace` keeps a single rolling backup file.
/// Payloads above the simple-upload limit go through a resumable session.
async fn upload_backup(
    cloud_env: CloudEnvironment,
    drive_id: &str,
    folder_id: &str,
    token: &str,
    payload: &serde_json::Value,
) -> Result<(), AppError> {
    let client = GraphClient::new(cloud_env);
    let body = serde_json::to_vec(payload).map_err(|e| AppError::Config {
        message: format!("backup payload serialize failed: {}", e),
    })?;

    if body.len() > SIMPLE_UPLOAD_LIMIT {
        upload_backup_session(&client, drive_id, folder_id, token, &body).await
    } else {
        let url = backup_upload_url(client.base_url(), drive_id, folder_id);
        client
            .request_with_retry(token, |http, tkn| build_put(http, &url, tkn, body.clone()))
            .await?;
        Ok(())
    }
}

/// Resumable upload: create a session, then PUT fixed-size chunks with
/// Content-Range headers. The session URL is pre-authenticated, so the
/// chunk PUTs carry no bearer token. A dedicated client with a long timeout
/// is used because 4 MiB chunks can be slow on uplink-limited connections.
async fn upload_backup_session(
    client: &GraphClient,
    drive_id: &str,
    folder_id: &str,
    token: &str,
    body: &[u8],
) -> Result<(), AppError> {
    let session_url = backup_session_url(client.base_url(), drive_id, folder_id);
    let response = client
        .request_with_retry(token, |http, tkn| {
            http.post(&session_url)
                .bearer_auth(tkn)
                .json(&json!({ "item": { "@microsoft.graph.conflictBehavior": "replace" } }))
        })
        .await?;
    let session: serde_json::Value = response.json().await.map_err(|e| AppError::GraphApi {
        message: format!("failed to parse upload session: {}", e),
        status_code: 0,
    })?;
    let upload_url = session["uploadUrl"]
        .as_str()
        .ok_or_else(|| AppError::GraphApi {
            message: "upload session response missing uploadUrl".to_string(),
            status_code: 0,
        })?
        .to_string();

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| AppError::GraphApi {
            message: format!("failed to build upload client: {}", e),
            status_code: 0,
        })?;

    let total = body.len();
    let mut start = 0usize;
    while start < total {
        let end = (start + SESSION_CHUNK_SIZE).min(total);
        let range = format!("bytes {}-{}/{}", start, end - 1, total);
        let response = http
            .put(&upload_url)
            .header("Content-Range", &range)
            .header("Content-Length", end - start)
            .body(body[start..end].to_vec())
            .send()
            .await
            .map_err(|e| AppError::GraphApi {
                message: format!("chunk upload failed: {}", e),
                status_code: 0,
            })?;
        let status = response.status().as_u16();
        if status != 206 && status != 200 && status != 201 {
            let text = response.text().await.unwrap_or_default();
            return Err(AppError::GraphApi {
                message: format!("chunk upload failed with status {}: {}", status, text),
                status_code: status,
            });
        }
        start = end;
    }
    Ok(())
}

/// Assemble + encrypt the full payload and upload it with the target
/// account's token. Startup/Exit triggers are gated on the dynamic sections
/// (chat history / catalog) having changed since the last upload — when
/// nothing changed, this returns without uploading.
async fn run_backup_upload(
    handle: AppHandle,
    target: AutoBackupTarget,
    base_payload: serde_json::Value,
    trigger: BackupTrigger,
    wait_for_session: bool,
) {
    let auth_state = match handle.try_state::<Mutex<AuthModule>>() {
        Some(state) => state,
        // AuthModule not managed yet (very early startup) — the backup
        // re-runs on the next save trigger.
        None => return,
    };

    // The startup trigger can race `restore_sessions`; give the session
    // one grace period before giving up.
    if wait_for_session
        && !auth_state
            .lock()
            .await
            .has_session(&target.cloud_env, &target.home_account_id)
    {
        tokio::time::sleep(SESSION_WAIT).await;
    }

    let token = {
        let mut auth = auth_state.lock().await;
        match auth
            .get_token_for_account(target.cloud_env.clone(), &target.home_account_id)
            .await
        {
            Ok(token) => token,
            Err(e) => {
                eprintln!("[config] auto backup token failed: {}", e);
                return;
            }
        }
    };

    let (payload, fingerprint) =
        backup_snapshot::assemble_backup_payload_with_fingerprint(&handle, base_payload);
    if trigger.gated_on_dynamic_change() {
        match &fingerprint {
            // No dynamic content selected or collected → the startup/exit
            // triggers have nothing to add beyond the Change triggers.
            None => return,
            Some(fp) => {
                let unchanged = handle
                    .path()
                    .app_data_dir()
                    .ok()
                    .and_then(|dir| backup_snapshot::load_dynamic_hash(&dir))
                    .as_deref()
                    == Some(fp.as_str());
                if unchanged {
                    return;
                }
            }
        }
    }

    if let Err(e) = upload_backup(
        target.cloud_env.clone(),
        &target.drive_id,
        &target.folder_id,
        &token,
        &payload,
    )
    .await
    {
        eprintln!("[config] auto backup upload failed: {}", e);
        return;
    }

    // Record what the cloud now holds so the next Startup/Exit trigger can
    // skip unchanged content.
    if let (Some(fp), Ok(dir)) = (
        fingerprint,
        handle.path().app_data_dir(),
    ) {
        backup_snapshot::save_dynamic_hash(&dir, &fp);
    }
}

/// Spawn the fire-and-forget backup upload for one trigger (config save,
/// account save, startup restore). Failures are logged, never surfaced —
/// matches the old local-file behavior of not blocking saves.
pub fn spawn_auto_backup_upload(
    handle: AppHandle,
    target: AutoBackupTarget,
    base_payload: serde_json::Value,
    trigger: BackupTrigger,
) {
    tauri::async_runtime::spawn(run_backup_upload(
        handle,
        target,
        base_payload,
        trigger,
        true,
    ));
}

/// Exit-time variant: runs the same upload inline with a shutdown timeout so
/// the final state (e.g. the last chat messages) still reaches the cloud.
pub fn run_auto_backup_blocking(
    handle: AppHandle,
    target: AutoBackupTarget,
    base_payload: serde_json::Value,
    trigger: BackupTrigger,
) {
    let _ = tauri::async_runtime::block_on(async move {
        tokio::time::timeout(
            BLOCKING_BACKUP_TIMEOUT,
            run_backup_upload(handle, target, base_payload, trigger, true),
        )
        .await
    });
}

/// Awaited variant used by `set_auto_backup_target`: performs the first
/// backup upload immediately so the user gets direct feedback that the
/// chosen destination works. Errors propagate to the caller.
pub async fn upload_backup_now(
    handle: &AppHandle,
    target: &AutoBackupTarget,
    base_payload: &serde_json::Value,
) -> Result<(), AppError> {
    let auth_state = handle.state::<Mutex<AuthModule>>();
    let token = {
        let mut auth = auth_state.lock().await;
        auth.get_token_for_account(target.cloud_env.clone(), &target.home_account_id)
            .await?
    };
    let (payload, fingerprint) = backup_snapshot::assemble_backup_payload_with_fingerprint(
        handle,
        base_payload.clone(),
    );
    upload_backup(
        target.cloud_env.clone(),
        &target.drive_id,
        &target.folder_id,
        &token,
        &payload,
    )
    .await?;
    if let (Some(fp), Ok(dir)) = (fingerprint, handle.path().app_data_dir()) {
        backup_snapshot::save_dynamic_hash(&dir, &fp);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_upload_url_puts_conflict_behavior_in_query() {
        assert_eq!(
            backup_upload_url(
                "https://graph.microsoft.com/v1.0",
                "drive-1",
                "folder-9"
            ),
            "https://graph.microsoft.com/v1.0/drives/drive-1/items/folder-9:/ShareOneList-backup.json:/content?@microsoft.graph.conflictBehavior=replace"
        );
        assert!(backup_upload_url("https://microsoftgraph.chinacloudapi.cn/v1.0", "d", "f")
            .starts_with("https://microsoftgraph.chinacloudapi.cn/v1.0/drives/d/items/f:/"));
    }

    #[test]
    fn session_url_targets_create_upload_session() {
        assert_eq!(
            backup_session_url("https://graph.microsoft.com/v1.0", "d", "f"),
            "https://graph.microsoft.com/v1.0/drives/d/items/f:/ShareOneList-backup.json:/createUploadSession"
        );
    }

    #[tokio::test]
    async fn put_request_builds_with_query_intact() {
        let http = reqwest::Client::new();
        let url = backup_upload_url("https://graph.microsoft.com/v1.0", "d", "f");
        let req = build_put(&http, &url, "tok", b"{}".to_vec())
            .build()
            .expect("backup PUT must build");
        assert_eq!(req.method(), reqwest::Method::PUT);
        assert_eq!(
            req.url().query(),
            Some("@microsoft.graph.conflictBehavior=replace")
        );
        assert_eq!(
            req.headers().get("Content-Type").and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
    }

    #[tokio::test]
    async fn conflict_behavior_is_not_a_legal_header_name() {
        // Documents why the query parameter is used: `@` is outside the
        // RFC 7230 token charset, so such a request can never be sent.
        let http = reqwest::Client::new();
        assert!(http
            .put("https://example.invalid/")
            .header("@microsoft.graph.conflictBehavior", "replace")
            .build()
            .is_err());
    }
}
