use serde::Serialize;
use tauri::State;

use crate::config::backup_crypto;
use crate::config::backup_snapshot;
use crate::config::ConfigManager;
use crate::errors::AppError;
use crate::models::{AccountEntry, AppConfig, BackupItem};

/// What an import restored, reported back to the UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub config: AppConfig,
    pub accounts: Vec<AccountEntry>,
    pub accounts_count: usize,
    /// Optional sections that were actually restored from the file.
    pub items_restored: Vec<BackupItem>,
}

/// Header info of a backup file, for the import confirmation UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInspectInfo {
    pub backup_version: i64,
    pub exported_at: String,
    pub items: Vec<BackupItem>,
    pub encrypted: bool,
}

/// Whether a backup password has been set on this machine.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPasswordState {
    pub is_set: bool,
}

/// Retrieve the current application configuration from disk.
///
/// Returns the persisted `AppConfig`, falling back to defaults if the
/// config file is missing or contains invalid JSON.
#[tauri::command]
pub fn get_config(config_manager: State<'_, ConfigManager>) -> AppConfig {
    config_manager.load_config()
}

/// Persist the given application configuration to disk.
///
/// Writes the `AppConfig` as JSON to the platform app data directory.
#[tauri::command]
pub fn save_config(
    config: AppConfig,
    config_manager: State<'_, ConfigManager>,
) -> Result<(), AppError> {
    config_manager
        .save_config(&config)
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })
}

/// Retrieve the list of persisted account entries from disk.
///
/// Returns an empty `Vec` if the accounts file is missing or invalid.
#[tauri::command]
pub fn get_accounts(config_manager: State<'_, ConfigManager>) -> Vec<AccountEntry> {
    config_manager.load_accounts()
}

/// Export settings + accounts plus the selected optional sections (the
/// secret-bearing ones encrypted with the locally cached backup key) to a
/// local file. Mirrors what the automatic cloud backup uploads.
#[tauri::command]
pub fn export_config(
    path: String,
    config_manager: State<'_, ConfigManager>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let payload = config_manager
        .build_backup_payload()
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
    let payload = backup_snapshot::assemble_backup_payload(&app, payload);
    config_manager
        .write_payload(std::path::Path::new(&path), &payload)
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })
}

/// Header of a backup file: version, export time, which optional sections
/// it carries and whether they are encrypted. Lets the UI ask for a
/// password only when one is actually needed.
#[tauri::command]
pub fn inspect_backup(
    path: String,
    config_manager: State<'_, ConfigManager>,
) -> Result<BackupInspectInfo, AppError> {
    let payload = config_manager
        .read_backup_payload(std::path::Path::new(&path))
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
    let items: Vec<BackupItem> = payload
        .get("items")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    Ok(BackupInspectInfo {
        backup_version: payload.get("backupVersion").and_then(|v| v.as_i64()).unwrap_or(1),
        exported_at: payload
            .get("exportedAt")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        encrypted: payload.get("enc").map(|v| !v.is_null()).unwrap_or(false),
        items,
    })
}

/// Import a backup file: restore the requested optional sections (defaults
/// to every section present in the file), then overwrite config + accounts.
/// Encrypted sections require the backup password; a wrong password aborts
/// the import before anything persistent changes.
#[tauri::command]
pub async fn import_config(
    path: String,
    password: Option<String>,
    items: Option<Vec<BackupItem>>,
    config_manager: State<'_, ConfigManager>,
    auth_module: State<'_, tokio::sync::Mutex<crate::auth::AuthModule>>,
    app: tauri::AppHandle,
) -> Result<ImportSummary, AppError> {
    let payload = config_manager
        .read_backup_payload(std::path::Path::new(&path))
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;

    let present: Vec<BackupItem> = payload
        .get("items")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let encrypted = payload.get("enc").map(|v| !v.is_null()).unwrap_or(false);
    let to_restore: Vec<BackupItem> = match items {
        Some(requested) => present.into_iter().filter(|i| requested.contains(i)).collect(),
        None => present,
    };

    // Optional sections first: any failure here aborts before config or
    // accounts are overwritten.
    if !to_restore.is_empty() && encrypted {
        let password = password.filter(|p| !p.is_empty()).ok_or_else(|| AppError::Config {
            message: "this backup is encrypted — the backup password is required".to_string(),
        })?;
        let section: backup_crypto::EncryptedSection = serde_json::from_value(
            payload.get("enc").cloned().unwrap_or(serde_json::Value::Null),
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        let sections = backup_crypto::decrypt_sections(&password, &section).map_err(|e| {
            AppError::Config {
                message: e.to_string(),
            }
        })?;
        backup_snapshot::restore_sections(&app, &sections, &to_restore).map_err(|e| {
            AppError::Config {
                message: e,
            }
        })?;
    }

    let (config, accounts, count) =
        config_manager
            .apply_base_import(&payload)
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;

    // Materialize sessions for accounts whose refresh tokens were just
    // restored — they come back to life without a re-login. Accounts
    // without a usable stored credential log a failure and stay pending.
    {
        let mut auth = auth_module.lock().await;
        auth.restore_sessions(accounts.clone()).await;
    }

    Ok(ImportSummary {
        config,
        accounts,
        accounts_count: count,
        items_restored: to_restore,
    })
}

/// Set (or replace) the backup password. Generates a fresh salt and caches
/// the derived key in the OS keyring so automatic backups can encrypt
/// without prompting. Restores on other machines will need this password.
#[tauri::command]
pub fn set_backup_password(password: String) -> Result<(), AppError> {
    if password.chars().count() < 6 {
        return Err(AppError::Validation {
            field: "password".to_string(),
            message: "backup password must be at least 6 characters".to_string(),
        });
    }
    backup_crypto::set_backup_password(&password)
        .map(|_| ())
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })
}

/// Whether a backup password has been set on this machine.
#[tauri::command]
pub fn get_backup_password_state() -> BackupPasswordState {
    BackupPasswordState {
        is_set: backup_crypto::has_backup_key(),
    }
}

/// Set the cloud auto backup target (None disables).
///
/// When a target is given, the first backup is uploaded *before* the config
/// is saved: a destination that cannot be written is rejected with an error
/// and nothing is persisted. The save itself then triggers the usual
/// fire-and-forget upload carrying the final config.
#[tauri::command]
pub async fn set_auto_backup_target(
    target: Option<crate::models::AutoBackupTarget>,
    config_manager: State<'_, ConfigManager>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let target = target.filter(|t| {
        !t.home_account_id.trim().is_empty()
            && !t.drive_id.trim().is_empty()
            && !t.folder_id.trim().is_empty()
    });

    if let Some(target) = &target {
        let payload = config_manager
            .build_backup_payload()
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        crate::config::cloud_backup::upload_backup_now(&app, target, &payload).await?;
    }

    let mut config = config_manager.load_config();
    config.auto_backup_target = target;
    config_manager
        .save_config(&config)
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })
}
