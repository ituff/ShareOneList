// Mount point definitions and persistence for the WebDAV gateway.
//
// Mounts live in `webdav-mounts.json` next to the other config files, NOT in
// config.json: the gateway credential is stored alongside them and must never
// leak into the config export/backup payload.

use std::fs;
use std::path::{Path, PathBuf};

use rand::Rng;

use crate::auth::cloud_config::CloudEnvironment;

const MOUNTS_FILE_NAME: &str = "webdav-mounts.json";
/// User name presented to the OS WebDAV client. A single global credential
/// pair guards the whole gateway: Windows allows only one credential set per
/// server, so per-mount passwords would fail the second mount with
/// ERROR_SESSION_CREDENTIAL_CONFLICT (1219).
pub const GATEWAY_USERNAME: &str = "shareonelist";

/// One mountable cloud location bound to a single account (session isolation
/// comes from (cloud_env, home_account_id), matching AuthModule's keying).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MountEntry {
    /// Short random id used in the gateway URL path.
    pub mount_id: String,
    pub cloud_env: CloudEnvironment,
    pub home_account_id: String,
    pub drive_id: String,
    /// Graph item id of the exposed subtree; empty string = drive root.
    #[serde(default)]
    pub root_item_id: String,
    pub label: String,
    /// Preferred Windows drive letter (e.g. "Z"); None = auto-pick at mount time.
    #[serde(default)]
    pub drive_letter: Option<String>,
}

/// Persisted gateway config: mount list + the gateway Basic-auth password.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MountConfig {
    pub mounts: Vec<MountEntry>,
    /// Loopback-only secret; guards the gateway, never a Graph credential.
    pub gateway_password: String,
    /// Preferred port; the listener walks upward from here when busy and
    /// writes the bound port back so OS-side remembered URLs keep working.
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_port() -> u16 {
    3980
}

impl Default for MountConfig {
    fn default() -> Self {
        Self {
            mounts: Vec::new(),
            gateway_password: generate_password(),
            port: default_port(),
        }
    }
}

/// 32-byte random password, hex encoded (64 chars).
pub fn generate_password() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Loads and saves `webdav-mounts.json` with the same tolerate-corruption
/// policy as the rest of the config layer: a missing or invalid file falls
/// back to a fresh default (which regenerates the gateway password).
pub struct MountStore {
    path: PathBuf,
}

impl MountStore {
    pub fn new(base_path: &Path) -> Self {
        Self {
            path: base_path.join(MOUNTS_FILE_NAME),
        }
    }

    pub fn load(&self) -> MountConfig {
        match fs::read_to_string(&self.path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => MountConfig::default(),
        }
    }

    pub fn save(&self, config: &MountConfig) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
        fs::write(&self.path, json).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_file_returns_default_with_password() {
        let dir = tempfile::tempdir().unwrap();
        let store = MountStore::new(dir.path());
        let config = store.load();
        assert!(config.mounts.is_empty());
        assert_eq!(config.port, 3980);
        assert_eq!(config.gateway_password.len(), 64);
    }

    #[test]
    fn load_invalid_json_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MOUNTS_FILE_NAME), "not json {{{").unwrap();
        let store = MountStore::new(dir.path());
        let config = store.load();
        assert!(config.mounts.is_empty());
        assert!(!config.gateway_password.is_empty());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = MountStore::new(dir.path());
        let mut config = MountConfig::default();
        config.mounts.push(MountEntry {
            mount_id: "abc123".to_string(),
            cloud_env: CloudEnvironment::China,
            home_account_id: "acc".to_string(),
            drive_id: "drive".to_string(),
            root_item_id: String::new(),
            label: "站点/文档库".to_string(),
            drive_letter: Some("Z".to_string()),
        });
        config.port = 3990;
        store.save(&config).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.port, 3990);
        assert_eq!(loaded.mounts.len(), 1);
        assert_eq!(loaded.mounts[0].label, "站点/文档库");
        assert_eq!(loaded.mounts[0].drive_letter.as_deref(), Some("Z"));
        assert_eq!(loaded.gateway_password, config.gateway_password);
    }

    #[test]
    fn generated_passwords_are_unique_and_hex() {
        let a = generate_password();
        let b = generate_password();
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
