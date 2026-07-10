// Config module: persistent configuration and account storage
// Uses platform-appropriate app data directories

pub mod commands;

use std::fs;
use std::path::PathBuf;

use crate::models::{AccountEntry, AppConfig};

const CONFIG_FILE_NAME: &str = "config.json";
const ACCOUNTS_FILE_NAME: &str = "accounts.json";

/// Manages persistent configuration and account storage on disk.
///
/// `ConfigManager` reads/writes JSON files from a base directory (typically
/// the platform app data dir). It gracefully handles missing or invalid files
/// by falling back to defaults.
pub struct ConfigManager {
    config_path: PathBuf,
    accounts_path: PathBuf,
}

impl ConfigManager {
    /// Create a new `ConfigManager` rooted at the given base directory.
    ///
    /// The base path should be the platform app data directory
    /// (e.g. `AppData\Roaming\<app>` on Windows, `~/Library/Application Support/<app>` on macOS).
    pub fn new(base_path: PathBuf) -> Self {
        Self {
            config_path: base_path.join(CONFIG_FILE_NAME),
            accounts_path: base_path.join(ACCOUNTS_FILE_NAME),
        }
    }

    /// Load application configuration from disk.
    ///
    /// Returns `AppConfig::default()` if the file does not exist or contains invalid JSON.
    pub fn load_config(&self) -> AppConfig {
        match fs::read_to_string(&self.config_path) {
            Ok(content) => {
                serde_json::from_str(&content).unwrap_or_default()
            }
            Err(_) => AppConfig::default(),
        }
    }

    /// Persist application configuration to disk as JSON.
    ///
    /// Creates the parent directory if it does not exist.
    pub fn save_config(&self, config: &AppConfig) -> Result<(), ConfigError> {
        self.ensure_dir_exists(&self.config_path)?;
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| ConfigError::Serialize(e.to_string()))?;
        fs::write(&self.config_path, json)
            .map_err(|e| ConfigError::Io(e.to_string()))?;
        Ok(())
    }

    /// Load persisted account entries from disk.
    ///
    /// Returns an empty `Vec` if the file does not exist or contains invalid JSON.
    pub fn load_accounts(&self) -> Vec<AccountEntry> {
        match fs::read_to_string(&self.accounts_path) {
            Ok(content) => {
                serde_json::from_str(&content).unwrap_or_default()
            }
            Err(_) => Vec::new(),
        }
    }

    /// Persist account entries to disk as JSON.
    ///
    /// Creates the parent directory if it does not exist.
    pub fn save_accounts(&self, accounts: &[AccountEntry]) -> Result<(), ConfigError> {
        self.ensure_dir_exists(&self.accounts_path)?;
        let json = serde_json::to_string_pretty(accounts)
            .map_err(|e| ConfigError::Serialize(e.to_string()))?;
        fs::write(&self.accounts_path, json)
            .map_err(|e| ConfigError::Io(e.to_string()))?;
        Ok(())
    }

    /// Ensure the parent directory of `file_path` exists, creating it if necessary.
    fn ensure_dir_exists(&self, file_path: &PathBuf) -> Result<(), ConfigError> {
        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)
                    .map_err(|e| ConfigError::Io(e.to_string()))?;
            }
        }
        Ok(())
    }
}

/// Errors that can occur during configuration operations.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("Serialization error: {0}")]
    Serialize(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ThemeMode, WindowState};
    use crate::auth::cloud_config::CloudEnvironment;
    use tempfile::TempDir;

    fn make_manager(dir: &TempDir) -> ConfigManager {
        ConfigManager::new(dir.path().to_path_buf())
    }

    #[test]
    fn load_config_returns_default_when_file_missing() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);
        let config = mgr.load_config();
        assert_eq!(config.theme, ThemeMode::System);
        assert_eq!(config.language, "en-US");
        assert_eq!(config.window.width, 1280);
        assert_eq!(config.window.height, 720);
    }

    #[test]
    fn load_config_returns_default_for_invalid_json() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);
        fs::write(mgr.config_path.clone(), "not valid json {{{").unwrap();
        let config = mgr.load_config();
        assert_eq!(config.theme, ThemeMode::System);
        assert_eq!(config.language, "en-US");
    }

    #[test]
    fn save_and_load_config_round_trip() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);

        let config = AppConfig {
            theme: ThemeMode::Dark,
            language: "zh-CN".to_string(),
            window: WindowState {
                x: 100,
                y: 200,
                width: 1920,
                height: 1080,
                is_maximized: true,
            },
        };

        mgr.save_config(&config).unwrap();
        let loaded = mgr.load_config();

        assert_eq!(loaded.theme, ThemeMode::Dark);
        assert_eq!(loaded.language, "zh-CN");
        assert_eq!(loaded.window.x, 100);
        assert_eq!(loaded.window.y, 200);
        assert_eq!(loaded.window.width, 1920);
        assert_eq!(loaded.window.height, 1080);
        assert!(loaded.window.is_maximized);
    }

    #[test]
    fn load_accounts_returns_empty_when_file_missing() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);
        let accounts = mgr.load_accounts();
        assert!(accounts.is_empty());
    }

    #[test]
    fn load_accounts_returns_empty_for_invalid_json() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);
        fs::write(mgr.accounts_path.clone(), "broken!!!").unwrap();
        let accounts = mgr.load_accounts();
        assert!(accounts.is_empty());
    }

    #[test]
    fn save_and_load_accounts_round_trip() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);

        let accounts = vec![
            AccountEntry {
                home_account_id: "user-1".to_string(),
                drive_id: "drive-1".to_string(),
                cloud_type: CloudEnvironment::Global,
                display_name: "Test User".to_string(),
            },
            AccountEntry {
                home_account_id: "user-2".to_string(),
                drive_id: "drive-2".to_string(),
                cloud_type: CloudEnvironment::China,
                display_name: "测试用户".to_string(),
            },
        ];

        mgr.save_accounts(&accounts).unwrap();
        let loaded = mgr.load_accounts();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].home_account_id, "user-1");
        assert_eq!(loaded[0].cloud_type, CloudEnvironment::Global);
        assert_eq!(loaded[1].home_account_id, "user-2");
        assert_eq!(loaded[1].cloud_type, CloudEnvironment::China);
        assert_eq!(loaded[1].display_name, "测试用户");
    }

    #[test]
    fn save_config_creates_directory_if_missing() {
        let dir = TempDir::new().unwrap();
        let nested_path = dir.path().join("sub").join("dir");
        let mgr = ConfigManager::new(nested_path);

        let config = AppConfig::default();
        mgr.save_config(&config).unwrap();

        let loaded = mgr.load_config();
        assert_eq!(loaded.theme, ThemeMode::System);
    }

    #[test]
    fn save_accounts_creates_directory_if_missing() {
        let dir = TempDir::new().unwrap();
        let nested_path = dir.path().join("another").join("path");
        let mgr = ConfigManager::new(nested_path);

        mgr.save_accounts(&vec![]).unwrap();
        let loaded = mgr.load_accounts();
        assert!(loaded.is_empty());
    }
}
