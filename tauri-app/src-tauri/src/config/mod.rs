// Config module: persistent configuration and account storage
// Uses platform-appropriate app data directories

pub mod backup_crypto;
pub mod backup_snapshot;
pub mod cloud_backup;
pub mod commands;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::Utc;

use crate::auth::cloud_config::CloudEnvironment;
use crate::models::{AccountEntry, AppConfig, AutoBackupTarget};

const CONFIG_FILE_NAME: &str = "config.json";
const ACCOUNTS_FILE_NAME: &str = "accounts.json";
const LEGACY_MIGRATION_MARKER: &str = ".legacy-drives-migrated";

/// Receives `(target, payload, trigger)` for every auto backup trigger. The
/// app installs one at setup that uploads to OneDrive; unit tests either
/// install a recording sink or none at all. Keeping this a plain callback
/// keeps the config layer free of the Tauri runtime (and the test binary
/// free of its GUI imports).
type BackupSink = Box<
    dyn Fn(
            AutoBackupTarget,
            serde_json::Value,
            crate::config::cloud_backup::BackupTrigger,
        ) + Send
        + Sync,
>;

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
    /// Cloud auto backup sink, installed during app setup. Backups triggered
    /// before that (or in unit tests without a sink) are skipped.
    backup_sink: OnceLock<BackupSink>,
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
            backup_sink: OnceLock::new(),
        }
    }

    /// Install the callback that receives every auto backup trigger. Only the
    /// first installation takes effect.
    pub fn set_backup_sink(
        &self,
        sink: impl Fn(
                AutoBackupTarget,
                serde_json::Value,
                crate::config::cloud_backup::BackupTrigger,
            ) + Send
            + Sync
            + 'static,
    ) {
        let _ = self.backup_sink.set(Box::new(sink));
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

    /// Build the base export/auto-backup payload: settings + accounts.
    /// Version-2 payloads additionally carry `items` (which optional
    /// sections are included) and `enc` (those sections, encrypted) — both
    /// are attached by `backup_snapshot::assemble_backup_payload`, which
    /// needs the AppHandle to reach the other modules' state.
    pub fn build_backup_payload(&self) -> Result<serde_json::Value, ConfigError> {
        let payload = serde_json::json!({
            "kind": "ShareOneList-backup",
            "backupVersion": 2,
            "exportedAt": Utc::now().to_rfc3339(),
            "config": self.load_config(),
            "accounts": self.load_accounts(),
        });
        Ok(payload)
    }

    /// Write the backup payload to `path` as pretty JSON (atomically).
    pub(crate) fn write_payload(
        &self,
        path: &Path,
        payload: &serde_json::Value,
    ) -> Result<(), ConfigError> {
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

    /// Read and validate a backup file, returning the full payload. The
    /// optional sections (`enc`) are handled by the import command; this
    /// only checks the `kind` marker.
    pub fn read_backup_payload(&self, path: &Path) -> Result<serde_json::Value, ConfigError> {
        let content = fs::read_to_string(path).map_err(|e| ConfigError::Io(e.to_string()))?;
        let payload: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| ConfigError::Serialize(e.to_string()))?;
        if payload.get("kind").and_then(|v| v.as_str()) != Some("ShareOneList-backup") {
            return Err(ConfigError::Serialize(
                "not a ShareOneList backup file".to_string(),
            ));
        }
        Ok(payload)
    }

    /// Apply the base part of a backup payload: overwrite the current config
    /// and account list. (Optional sections are restored by the import
    /// command BEFORE this runs, so a section failure aborts the import
    /// while nothing persistent has changed.)
    pub fn apply_base_import(
        &self,
        payload: &serde_json::Value,
    ) -> Result<(AppConfig, Vec<AccountEntry>, usize), ConfigError> {
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

    /// Import settings + accounts from a backup file. Overwrites the current
    /// config and account list and returns what was imported. (Version-1
    /// path; the import command handles optional sections before calling
    /// this via `apply_base_import`.)
    pub fn import_bundle(
        &self,
        path: &Path,
    ) -> Result<(AppConfig, Vec<AccountEntry>, usize), ConfigError> {
        let payload = self.read_backup_payload(path)?;
        self.apply_base_import(&payload)
    }

    /// Hands the backup payload to the installed sink, which uploads it into
    /// the user-configured cloud OneDrive folder (fire-and-forget). No-op
    /// when no target is configured or no sink is installed yet.
    pub fn write_auto_backup(&self) {
        self.write_auto_backup_with_trigger(crate::config::cloud_backup::BackupTrigger::Change);
    }

    /// Same as [`write_auto_backup`] with an explicit trigger. The startup
    /// and exit triggers are gated on dynamic content (chat history /
    /// catalog) having changed — see `cloud_backup::BackupTrigger`.
    pub fn write_auto_backup_with_trigger(
        &self,
        trigger: crate::config::cloud_backup::BackupTrigger,
    ) {
        let Some(target) = self.load_config().auto_backup_target else {
            return;
        };
        if target.drive_id.is_empty() || target.folder_id.is_empty() {
            return;
        }
        let Some(sink) = self.backup_sink.get() else {
            // Early startup (first save_config in setup) — the post-restore
            // hook re-runs the backup once sessions are available.
            return;
        };
        match self.build_backup_payload() {
            Ok(payload) => sink(target, payload, trigger),
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
    use crate::config::cloud_backup::BackupTrigger;
    use crate::models::{AccountCategory, AutoBackupTarget, ThemeMode, WindowState};
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
            auto_backup_target: None,
            auto_backup_items: Vec::new(),
            close_to_tray: false,
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
            auto_backup_target: None,
            auto_backup_items: Vec::new(),
            close_to_tray: false,
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
    fn auto_backup_target_round_trips_through_config() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);

        let target = AutoBackupTarget {
            home_account_id: "acc-1".to_string(),
            cloud_env: CloudEnvironment::China,
            drive_id: "drive-1".to_string(),
            folder_id: "folder-9".to_string(),
            folder_path: "/Apps/ShareOneList".to_string(),
        };
        let mut config = mgr.load_config();
        config.auto_backup_target = Some(target.clone());
        mgr.save_config(&config).unwrap();

        let loaded = mgr.load_config();
        assert_eq!(loaded.auto_backup_target, Some(target));
    }

    #[test]
    fn legacy_auto_backup_dir_key_is_ignored() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);
        // Config files written by versions that used a local OneDrive sync
        // folder carry `autoBackupDir`; it must not be resurrected.
        fs::write(
            mgr.config_path.clone(),
            r#"{"theme":"system","language":"zh-CN","window":{"x":0,"y":0,"width":1280,"height":720,"isMaximized":false},"autoBackupDir":"C:/OneDrive"}"#,
        )
        .unwrap();

        let loaded = mgr.load_config();
        assert_eq!(loaded.auto_backup_target, None);
    }

    #[test]
    fn auto_backup_skipped_without_target_or_sink() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);

        // No target configured: no-op.
        mgr.write_auto_backup();

        // Target configured but no sink installed (unit-test manager): no-op,
        // must not panic — uploads only happen through the installed sink.
        let mut config = mgr.load_config();
        config.auto_backup_target = Some(AutoBackupTarget {
            home_account_id: "acc-1".to_string(),
            cloud_env: CloudEnvironment::Global,
            drive_id: "drive-1".to_string(),
            folder_id: "folder-1".to_string(),
            folder_path: "/backups".to_string(),
        });
        mgr.save_config(&config).unwrap();
        mgr.write_auto_backup();
    }

    #[test]
    fn auto_backup_forwards_target_and_payload_to_sink() {
        use std::sync::{Arc, Mutex};

        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);
        let received: Arc<Mutex<Vec<(AutoBackupTarget, serde_json::Value, BackupTrigger)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let sink_recv = Arc::clone(&received);
        mgr.set_backup_sink(move |target, payload, trigger| {
            sink_recv.lock().unwrap().push((target, payload, trigger));
        });

        // No target yet: saves must not reach the sink.
        mgr.save_accounts(&[]).unwrap();
        assert!(received.lock().unwrap().is_empty());

        // Configure the target (through the normal save path): the config
        // save itself triggers one backup, the following account save another.
        let target = AutoBackupTarget {
            home_account_id: "acc-1".to_string(),
            cloud_env: CloudEnvironment::China,
            drive_id: "drive-1".to_string(),
            folder_id: "folder-9".to_string(),
            folder_path: "/Apps/ShareOneList".to_string(),
        };
        let mut config = mgr.load_config();
        config.auto_backup_target = Some(target.clone());
        mgr.save_config(&config).unwrap();
        mgr.save_accounts(&[]).unwrap();

        let calls = received.lock().unwrap();
        assert_eq!(calls.len(), 2, "config save + accounts save should each trigger");
        assert_eq!(calls[0].0, target);
        assert_eq!(calls[0].2, BackupTrigger::Change);
        assert_eq!(
            calls[0].1.get("kind").and_then(|v| v.as_str()),
            Some("ShareOneList-backup")
        );
        assert!(calls[1].1.get("config").is_some());
        assert!(calls[1].1.get("accounts").is_some());
    }
}
