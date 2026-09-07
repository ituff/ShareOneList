use std::collections::HashMap;

use tauri::State;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::auth::AuthModule;
use crate::auth::cloud_config::CloudEnvironment;
use crate::catalog::query::{query_catalog, AccountScope, CatalogHit};
use crate::catalog::seed::{run_deep_index, run_seed};
use crate::catalog::store::{CatalogDrive, CatalogStore};
use std::sync::Arc;
use crate::catalog::writeback::{record_ai_read, record_browse, record_hits, BrowseItem, HitInput};
use crate::errors::AppError;

/// Cancellation tokens for in-flight manual deep indexes, keyed by
/// "account_id|drive_id". Shared with spawned tasks via AppHandle state.
pub type CatalogCancels = Arc<Mutex<HashMap<String, CancellationToken>>>;

fn parse_env(cloud_env: &str) -> Result<CloudEnvironment, AppError> {
    match cloud_env {
        "global" => Ok(CloudEnvironment::Global),
        "china" => Ok(CloudEnvironment::China),
        other => Err(AppError::Validation {
            message: format!("unknown cloud env: {other}"),
            field: "cloudEnv".into(),
        }),
    }
}

async fn token_for(
    account_id: &str,
    env: CloudEnvironment,
    auth: &State<'_, Mutex<AuthModule>>,
) -> Result<String, AppError> {
    let mut auth = auth.lock().await;
    auth.get_token_for_account(env, account_id).await
}

/// Registers a drive (idempotent) and kicks off the root-level seed for new
/// or failed drives. This is the main entry point for requirement 1 + 2.
#[tauri::command]
pub async fn catalog_register_drive(
    account_id: String,
    cloud_env: String,
    drive_id: String,
    kind: String,
    name: String,
    site_name: String,
    store: State<'_, Arc<CatalogStore>>,
    auth: State<'_, Mutex<AuthModule>>,
    app_handle: tauri::AppHandle,
) -> Result<(), AppError> {
    let env = parse_env(&cloud_env)?;
    let is_new = store.register_drive(&account_id, &env, &drive_id, &kind, &name, &site_name)?;

    // Seed when the drive has never completed indexing (new or failed).
    let status = store.drive_status(&account_id, &drive_id)?.unwrap_or_default();
    if is_new || status == "failed" || status == "queued" {
        let token = token_for(&account_id, env.clone(), &auth).await?;
        let store: Arc<CatalogStore> = store.inner().clone();
        // Background: registration must not block the UI on Graph calls.
        tauri::async_runtime::spawn(async move {
            if let Err(e) = run_seed(&store, &app_handle, &account_id, env, &drive_id, &token).await {
                eprintln!("[catalog] seed failed for {drive_id}: {e}");
            }
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn catalog_unregister_drive(
    account_id: String,
    drive_id: String,
    store: State<'_, Arc<CatalogStore>>,
) -> Result<(), AppError> {
    store.unregister_drive(&account_id, &drive_id)
}

#[tauri::command]
pub async fn catalog_status(store: State<'_, Arc<CatalogStore>>) -> Result<Vec<CatalogDrive>, AppError> {
    store.list_drives()
}

/// Browse writeback fired by the file browser after a successful listing.
#[tauri::command]
pub async fn catalog_record_browse(
    account_id: String,
    cloud_env: String,
    drive_id: String,
    folder_path: String,
    folder_name: String,
    folder_item_id: String,
    items: Vec<BrowseItem>,
    store: State<'_, Arc<CatalogStore>>,
) -> Result<(), AppError> {
    let env = parse_env(&cloud_env)?;
    record_browse(
        store.inner(),
        &account_id,
        &env,
        &drive_id,
        &folder_path,
        &folder_name,
        &folder_item_id,
        &items,
    )
}

/// Search/grounding hit writeback.
#[tauri::command]
pub async fn catalog_record_hits(
    account_id: String,
    cloud_env: String,
    drive_id: String,
    source: String,
    question: String,
    hits: Vec<HitInput>,
    store: State<'_, Arc<CatalogStore>>,
) -> Result<(), AppError> {
    let env = parse_env(&cloud_env)?;
    record_hits(
        store.inner(),
        &account_id,
        &env,
        &drive_id,
        &source,
        &question,
        &hits,
    )
}

/// AI file-read writeback.
#[tauri::command]
pub async fn catalog_record_ai_read(
    account_id: String,
    cloud_env: String,
    drive_id: String,
    path: String,
    item_id: String,
    name: String,
    desc: String,
    store: State<'_, Arc<CatalogStore>>,
) -> Result<(), AppError> {
    let env = parse_env(&cloud_env)?;
    record_ai_read(
        store.inner(),
        &account_id,
        &env,
        &drive_id,
        &path,
        &item_id,
        &name,
        &desc,
    )
}

/// FTS5 catalog query scoped to the given accounts (None = all).
#[tauri::command]
pub async fn catalog_query(
    keywords: Vec<String>,
    accounts: Option<Vec<crate::catalog::query::AccountKey>>,
    limit: Option<usize>,
    store: State<'_, Arc<CatalogStore>>,
) -> Result<Vec<CatalogHit>, AppError> {
    let scope: Option<AccountScope> = match accounts {
        None => None,
        Some(keys) => Some(
            keys.into_iter()
                .map(|k| {
                    let env = parse_env(&k.cloud_env)?;
                    Ok((k.account_id, env))
                })
                .collect::<Result<Vec<_>, AppError>>()?,
        ),
    };
    let store: Arc<CatalogStore> = store.inner().clone();
    tokio::task::spawn_blocking(move || {
        query_catalog(&store, &keywords, scope.as_ref(), limit.unwrap_or(20))
    })
    .await
    .map_err(|e| AppError::Config {
        message: e.to_string(),
    })?
}

/// Manual deep index (user-initiated). Returns when finished or cancelled.
#[tauri::command]
pub async fn catalog_reindex(
    account_id: String,
    cloud_env: String,
    drive_id: String,
    max_depth: Option<usize>,
    store: State<'_, Arc<CatalogStore>>,
    auth: State<'_, Mutex<AuthModule>>,
    cancels: State<'_, CatalogCancels>,
    app_handle: tauri::AppHandle,
) -> Result<usize, AppError> {
    let env = parse_env(&cloud_env)?;
    let token = token_for(&account_id, env.clone(), &auth).await?;
    let cancel = CancellationToken::new();
    let key = format!("{account_id}|{drive_id}");
    cancels.lock().await.insert(key.clone(), cancel.clone());
    let result = run_deep_index(
        store.inner(),
        &app_handle,
        &account_id,
        env,
        &drive_id,
        &token,
        max_depth.unwrap_or(crate::catalog::seed::DEFAULT_MAX_DEPTH),
        cancel,
    )
    .await;
    cancels.lock().await.remove(&key);
    result
}

/// Cancels an in-flight manual index; unknown drives are ignored.
#[tauri::command]
pub async fn catalog_cancel_index(
    account_id: String,
    drive_id: String,
    cancels: State<'_, CatalogCancels>,
) -> Result<(), AppError> {
    if let Some(cancel) = cancels.lock().await.remove(&format!("{account_id}|{drive_id}")) {
        cancel.cancel();
    }
    Ok(())
}
