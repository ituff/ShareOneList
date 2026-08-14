use tauri::State;

use crate::config::ConfigManager;
use crate::errors::AppError;
use crate::models::{AccountEntry, AppConfig};

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
