use serde::Serialize;
use tauri::State;

use crate::config::ConfigManager;
use crate::errors::AppError;
use crate::models::{AccountEntry, AppConfig};

/// What an import restored, reported back to the UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub config: AppConfig,
    pub accounts: Vec<AccountEntry>,
    pub accounts_count: usize,
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

/// Export settings + accounts (no credentials) to a backup file.
#[tauri::command]
pub fn export_config(path: String, config_manager: State<'_, ConfigManager>) -> Result<(), AppError> {
    config_manager.export_bundle(std::path::Path::new(&path)).map_err(|e| AppError::Config {
        message: e.to_string(),
    })
}

/// Import settings + accounts from a backup file, overwriting the current
/// values. Returns the imported summary so the UI can refresh its stores.
#[tauri::command]
pub fn import_config(
    path: String,
    config_manager: State<'_, ConfigManager>,
) -> Result<ImportSummary, AppError> {
    let (config, accounts, accounts_count) = config_manager
        .import_bundle(std::path::Path::new(&path))
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
    Ok(ImportSummary {
        config,
        accounts_count,
        accounts,
    })
}

/// Set the auto backup directory (None disables). Saving the config also
/// writes an immediate auto backup when a directory is configured.
#[tauri::command]
pub fn set_auto_backup_dir(
    dir: Option<String>,
    config_manager: State<'_, ConfigManager>,
) -> Result<(), AppError> {
    let mut config = config_manager.load_config();
    config.auto_backup_dir = dir.filter(|d| !d.trim().is_empty());
    config_manager
        .save_config(&config)
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })
}
