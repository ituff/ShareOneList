// Config module: persistent configuration and account storage
// Uses platform-appropriate app data directories

pub mod commands;

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::auth::cloud_config::CloudEnvironment;
use crate::models::{AccountEntry, AppConfig};

const CONFIG_FILE_NAME: &str = "config.json";
const ACCOUNTS_FILE_NAME: &str = "accounts.json";
const LEGACY_MIGRATION_MARKER: &str = ".legacy-drives-migrated";
const AUTO_BACKUP_FILE_NAME: &str = "ShareOneList-backup.json";

/// Legacy WinUI `cache/drives.json` shape.
#[derive(Debug, serde::Deserialize)]
struct LegacyDrive {
    #[serde(rename = "DisplayName")]
    display_name: Option<String>,
    #[serde(rename = "Provider")]
    provider: Option<LegacyProvider>,
}

#[derive(Debug, serde::Deserialize)]
struct LegacyProvider {
    #[serde(rename = "HomeAccountId")]
    home_account_id: Option<String>,
    #[serde(rename = "DriveId")]
    drive_id: Option<String>,
    #[serde(rename = "CloudType")]
    cloud_type: Option<u8>,
}

/// Manages persistent configuration and account storage on disk.
///
/// `ConfigManager` reads/writes JSON files from a base directory (typically
/// the platform app data dir). It gracefully handles missing or invalid files
/// by falling back to defaults.
pub struct ConfigManager {
    config_path: PathBuf,
    accounts_path: PathBuf,
    legacy_migration_marker: PathBuf,
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
            legacy_migration_marker: base_path.join(LEGACY_MIGRATION_MARKER),
        }
    }

    /// One-time migration from the old WinUI `cache/drives.json` into the new account list.
    ///
    /// The old app stored drives next to its executable, so we search the executable's
    /// ancestors and the known unpackaged app cache location. Migration is idempotent and
    /// guarded by a marker file so removed accounts are not resurrected on later launches.
    pub fn migrate_legacy_accounts(&self) {
        if self.legacy_migration_marker.exists() {
            return;
        }

        let Some(legacy_path) = Self::find_legacy_drives_file() else {
            return;
        };
        let Ok(content) = fs::read_to_string(legacy_path) else {
            return;
        };
        let Ok(legacy_drives) = serde_json::from_str::<Vec<LegacyDrive>>(&content) else {
            return;
        };
        if legacy_drives.is_empty() {
            return;
        }

        let mut accounts = self.load_accounts();
        let mut changed = false;

        for legacy in legacy_drives {
            let Some(provider) = legacy.provider else {
                continue;
            };
            let cloud_type = match provider.cloud_type {
                Some(0) => CloudEnvironment::Global,
                Some(1) => CloudEnvironment::China,
                _ => continue,
            };
            let home_account_id = provider.home_account_id.unwrap_or_default();
            let drive_id = provider.drive_id.unwrap_or_default();
            let display_name = legacy.display_name.unwrap_or_default();
            if display_name.trim().is_empty() {
                continue;
            }

            let existing = accounts.iter_mut().find(|account| {
                (!drive_id.is_empty() && account.drive_id == drive_id)
                    || (!home_account_id.is_empty() && account.home_account_id == home_account_id)
                    || (account.cloud_type == cloud_type && account.display_name == display_name)
            });

            if let Some(account) = existing {
                if account.display_name.is_empty() || account.display_name == "Unknown User" {
                    account.display_name = display_name.clone();
                    changed = true;
                }
                if account.drive_id.is_empty() && !drive_id.is_empty() {
                    account.drive_id = drive_id.clone();
                    changed = true;
                }
                if account.home_account_id.is_empty() && !home_account_id.is_empty() {
                    account.home_account_id = home_account_id.clone();
                    changed = true;
                }
            } else {
                accounts.push(AccountEntry {
                    home_account_id,
                    drive_id,
                    cloud_type,
                    display_name,
                    account_type: None,
                    alias: None,
                    icon: None,
                });
                changed = true;
            }
        }

        if changed {
            if self.save_accounts(&accounts).is_err() {
                return;
            }
        }
        let _ = fs::write(&self.legacy_migration_marker, "1");
    }

    /// Locate the old WinUI `drives.json` file.
    fn find_legacy_drives_file() -> Option<PathBuf> {
        let mut candidates = Vec::new();

        if let Ok(exe) = std::env::current_exe() {
            let mut parent = exe.parent();
            for _ in 0..6 {
                if let Some(dir) = parent {
                    candidates.push(dir.join("cache").join("drives.json"));
                    parent = dir.parent();
                }
            }
        }

        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(
                PathBuf::from(local_app_data)
                    .join("com.onelab.shareonelist")
                    .join("cache")
                    .join("drives.json"),
            );
        }

        candidates.into_iter().find(|path| path.is_file())
    }

    /// Load application configuration from disk.
    ///
    /// Returns `AppConfig::default()` if the file does not exist or contains invalid JSON.
    pub fn load_config(&self) -> AppConfig {
        match fs::read_to_string(&self.config_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
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
        fs::write(&self.config_path, json).map_err(|e| ConfigError::Io(e.to_string()))?;
        self.write_auto_backup();
        Ok(())
    }

    /// Load persisted account entries from disk.
    ///
    /// Returns an empty `Vec` if the file does not exist or contains invalid JSON.
    pub fn load_accounts(&self) -> Vec<AccountEntry> {
        match fs::read_to_string(&self.accounts_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
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
        fs::write(&self.accounts_path, json).map_err(|e| ConfigError::Io(e.to_string()))?;
        self.write_auto_backup();
        Ok(())
    }

    /// Ensure the parent directory of `file_path` exists, creating it if necessary.
    fn ensure_dir_exists(&self, file_path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| ConfigError::Io(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// Build the export/auto-backup payload: settings + accounts (no
    /// credentials — refresh tokens never leave the OS keyring).
    fn build_backup_payload(&self) -> Result<serde_json::Value, ConfigError> {
        let payload = serde_json::json!({
            "kind": "ShareOneList-backup",
            "backupVersion": 1,
            "exportedAt": Utc::now().to_rfc3339(),
            "config": self.load_config(),
            "accounts": self.load_accounts(),
        });
        Ok(payload)
    }

    /// Write the backup payload to `path` as pretty JSON (atomically).
    fn write_payload(&self, path: &Path, payload: &serde_json::Value) -> Result<(), ConfigError> {
        self.ensure_dir_exists(path)?;
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(payload)
            .map_err(|e| ConfigError::Serialize(e.to_string()))?;
        fs::write(&tmp, json).map_err(|e| ConfigError::Io(e.to_string()))?;
        fs::rename(&tmp, path).map_err(|e| ConfigError::Io(e.to_string()))?;
        Ok(())
    }

    /// Export settings + accounts to the given file path.
    pub fn export_bundle(&self, path: &Path) -> Result<(), ConfigError> {
        let payload = self.build_backup_payload()?;
        self.write_payload(path, &payload)
    }

    /// Import settings + accounts from a backup file. Overwrites the current
    /// config and account list and returns what was imported.
    pub fn import_bundle(
        &self,
        path: &Path,
    ) -> Result<(AppConfig, Vec<AccountEntry>, usize), ConfigError> {
        let content = fs::read_to_string(path).map_err(|e| ConfigError::Io(e.to_string()))?;
        let payload: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| ConfigError::Serialize(e.to_string()))?;
        if payload.get("kind").and_then(|v| v.as_str()) != Some("ShareOneList-backup") {
            return Err(ConfigError::Serialize(
                "not a ShareOneList backup file".to_string(),
            ));
        }

        let config: AppConfig = serde_json::from_value(
            payload
                .get("config")
                .cloned()
                .ok_or_else(|| ConfigError::Serialize("missing config".to_string()))?,
        )
        .map_err(|e| ConfigError::Serialize(format!("invalid config: {}", e)))?;
        let accounts: Vec<AccountEntry> = serde_json::from_value(
            payload
                .get("accounts")
                .cloned()
                .ok_or_else(|| ConfigError::Serialize("missing accounts".to_string()))?,
        )
        .map_err(|e| ConfigError::Serialize(format!("invalid accounts: {}", e)))?;
        let count = accounts.len();

        self.save_config(&config)?;
        self.save_accounts(&accounts)?;
        Ok((config, accounts, count))
    }

    /// Writes the backup payload into the user-configured auto backup
    /// directory (typically a OneDrive sync folder). No-op when the
    /// directory is not configured.
    pub fn write_auto_backup(&self) {
        let Some(dir) = self
            .load_config()
            .auto_backup_dir
            .filter(|d| !d.trim().is_empty())
        else {
            return;
        };
        match self.build_backup_payload() {
            Ok(payload) => {
                if let Err(e) =
                    self.write_payload(&PathBuf::from(dir).join(AUTO_BACKUP_FILE_NAME), &payload)
                {
                    eprintln!("[config] auto backup failed: {}", e);
                }
            }
            Err(e) => eprintln!("[config] auto backup payload failed: {}", e),
        }
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
    use crate::models::{AccountCategory, ThemeMode, WindowState};
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
        assert_eq!(config.language, "system");
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
        assert_eq!(config.language, "system");
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
            last_download_path: Some("C:/Downloads".to_string()),
            segment_download_concurrency: 8,
            update_channel: "stable".to_string(),
            auto_backup_dir: None,
        };

        mgr.save_config(&config).unwrap();

        let loaded_config = mgr.load_config();
        assert_eq!(loaded_config.segment_download_concurrency, 8);
        let loaded = mgr.load_config();

        assert_eq!(loaded.theme, ThemeMode::Dark);
        assert_eq!(loaded.language, "zh-CN");
        assert_eq!(loaded.window.x, 100);
        assert_eq!(loaded.window.y, 200);
        assert_eq!(loaded.window.width, 1920);
        assert_eq!(loaded.window.height, 1080);
        assert!(loaded.window.is_maximized);
        assert_eq!(loaded.last_download_path.as_deref(), Some("C:/Downloads"));
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
                account_type: Some(AccountCategory::Organization),
                alias: None,
                icon: None,
            },
            AccountEntry {
                home_account_id: "user-2".to_string(),
                drive_id: "drive-2".to_string(),
                cloud_type: CloudEnvironment::China,
                display_name: "测试用户".to_string(),
                account_type: None,
                alias: Some("我的网盘".to_string()),
                icon: Some("star-amber".to_string()),
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
        assert_eq!(loaded[1].alias.as_deref(), Some("我的网盘"));
        assert_eq!(loaded[1].icon.as_deref(), Some("star-amber"));
    }

    #[test]
    fn load_accounts_tolerates_entries_without_alias_and_icon() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);
        fs::write(
            mgr.accounts_path.clone(),
            r#"[{
                "homeAccountId": "user-1",
                "driveId": "drive-1",
                "cloudType": "global",
                "displayName": "Test User",
                "accountType": "personal"
            }]"#,
        )
        .unwrap();

        let loaded = mgr.load_accounts();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].alias, None);
        assert_eq!(loaded[0].icon, None);
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

    #[test]
    fn export_import_bundle_round_trip() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);
        let accounts = vec![AccountEntry {
            home_account_id: "user-1".to_string(),
            drive_id: "drive-1".to_string(),
            cloud_type: CloudEnvironment::Global,
            display_name: "Test User".to_string(),
            account_type: Some(AccountCategory::Personal),
            alias: Some("My Drive".to_string()),
            icon: Some("star-amber".to_string()),
        }];

        mgr.save_config(&AppConfig {
            theme: ThemeMode::Dark,
            language: "zh-CN".to_string(),
            window: WindowState::default(),
            last_download_path: None,
            segment_download_concurrency: 8,
            update_channel: "beta".to_string(),
            auto_backup_dir: None,
        })
        .unwrap();
        mgr.save_accounts(&accounts).unwrap();

        let export_path = dir.path().join("backup.json");
        mgr.export_bundle(&export_path).unwrap();

        // Import into a SECOND manager pointing at another data dir, to
        // prove the round trip restores state elsewhere.
        let target = TempDir::new().unwrap();
        let target_mgr = ConfigManager::new(target.path().to_path_buf());
        let (config, imported, count) = target_mgr.import_bundle(&export_path).unwrap();

        assert_eq!(count, 1);
        assert_eq!(config.theme, ThemeMode::Dark);
        assert_eq!(config.update_channel, "beta");
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].alias.as_deref(), Some("My Drive"));
        assert_eq!(target_mgr.load_config().theme, ThemeMode::Dark);
        assert_eq!(target_mgr.load_accounts()[0].icon.as_deref(), Some("star-amber"));
    }

    #[test]
    fn import_bundle_rejects_non_backup_json() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);
        let not_backup = dir.path().join("not-backup.json");
        fs::write(&not_backup, "{\"hello\": 1}").unwrap();

        assert!(mgr.import_bundle(&not_backup).is_err());
    }

    #[test]
    fn auto_backup_writes_payload_when_dir_configured() {
        let dir = TempDir::new().unwrap();
        let backup_dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);
        let backup_path = backup_dir.path().join("ShareOneList-backup.json");

        // No backup dir configured yet: nothing written.
        mgr.save_accounts(&[]).unwrap();
        assert!(!backup_path.exists());

        // Configure the auto backup dir (through the normal save path) and
        // change accounts again — the backup must appear.
        let mut config = mgr.load_config();
        config.auto_backup_dir = Some(backup_dir.path().to_string_lossy().to_string());
        mgr.save_config(&config).unwrap();
        assert!(backup_path.exists(), "config save should trigger auto backup");

        mgr.save_accounts(&[]).unwrap();
        let payload: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&backup_path).unwrap()).unwrap();
        assert_eq!(
            payload.get("kind").and_then(|v| v.as_str()),
            Some("ShareOneList-backup")
        );
        assert!(payload.get("config").is_some());
        assert!(payload.get("accounts").is_some());
    }

    #[test]
    fn auto_backup_skipped_when_dir_empty() {
        let dir = TempDir::new().unwrap();
        let backup_dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);
        let backup_path = backup_dir.path().join("ShareOneList-backup.json");

        let mut config = mgr.load_config();
        config.auto_backup_dir = Some("   ".to_string());
        mgr.save_config(&config).unwrap();

        assert!(!backup_path.exists());
    }
}
