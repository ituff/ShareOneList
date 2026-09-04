use serde::{Deserialize, Serialize};

use crate::auth::cloud_config::CloudEnvironment;

/// Drive item representing a file or folder in OneDrive/SharePoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveItem {
    pub id: String,
    pub name: String,
    pub size: Option<u64>,
    pub last_modified: String, // ISO 8601
    pub is_folder: bool,
    pub mime_type: Option<String>,
    pub web_url: Option<String>,
    pub parent_reference: Option<ParentReference>,
    pub download_url: Option<String>,
    pub created_date_time: Option<String>,
}

/// Reference to a parent item/drive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentReference {
    pub drive_id: String,
    pub id: String,
    pub path: Option<String>,
    pub name: Option<String>,
}

/// Represents a drive (OneDrive personal, OneDrive for Business, or SharePoint document library).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    pub id: String,
    pub name: String,
    pub drive_type: String,
    pub quota: Option<DriveQuota>,
}

/// Storage quota information for a drive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveQuota {
    pub total: u64,
    pub used: u64,
    pub remaining: u64,
}

/// A SharePoint site.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Site {
    pub id: String,
    pub display_name: String,
    pub web_url: String,
}

/// Where a Teams meeting recording was discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordingSource {
    /// Organizer's OneDrive `Recordings` folder.
    OneDrive,
    /// A SharePoint site document library `Recordings` folder (channel meetings).
    SharePoint,
    /// Found via Microsoft Search across all content the user can access,
    /// covering participant-visible recordings on other people's drives.
    Search,
}

/// A Teams meeting recording aggregated across the user's drives.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingRecording {
    /// Drive holding the recording file; used by the frontend for thumbnails/downloads.
    pub drive_id: String,
    /// The recording file itself.
    pub item: DriveItem,
    pub source_type: RecordingSource,
    /// Display name of the originating site; empty for OneDrive recordings.
    pub source_name: String,
}

/// Transfer progress emitted via Tauri events.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub task_id: String,
    pub file_name: String,
    pub status: String, // "downloading" | "uploading" | "paused" | "completed" | "failed"
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub speed_bps: u64,
    pub elapsed_secs: f64,
    pub error: Option<String>,
    pub local_path: Option<String>,
}

/// Application configuration persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub theme: ThemeMode,
    pub language: String,
    pub window: WindowState,
    #[serde(default)]
    #[serde(alias = "last_download_path")]
    pub last_download_path: Option<String>,
}

/// Theme mode preference.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[serde(alias = "Light")]
    Light,
    #[serde(alias = "Dark")]
    Dark,
    #[serde(alias = "System")]
    System,
}

/// Window position and size state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(alias = "is_maximized")]
    pub is_maximized: bool,
}

/// Whether a Microsoft identity is a consumer (personal MSA) or an
/// organizational (work/school Entra) account. Only meaningful for the Global
/// cloud; 21Vianet sign-in is organizational by definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountCategory {
    Personal,
    Organization,
}

/// A persisted account entry linking a user to a drive and cloud environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountEntry {
    #[serde(alias = "home_account_id")]
    pub home_account_id: String,
    #[serde(alias = "drive_id")]
    pub drive_id: String,
    #[serde(alias = "cloud_type")]
    pub cloud_type: CloudEnvironment,
    #[serde(alias = "display_name")]
    pub display_name: String,
    /// Personal vs organizational identity; `None` for legacy entries and 21Vianet.
    #[serde(default, alias = "account_type")]
    pub account_type: Option<AccountCategory>,
}

/// Options for creating a share link.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareOptions {
    pub link_type: String,          // "view" | "edit"
    pub expiration: Option<String>, // ISO 8601 date
    pub password: Option<String>,
}

/// Configuration for pushing a download to an external downloader.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalDownloaderConfig {
    pub downloader_type: DownloaderType,
    pub rpc_url: String,
    pub secret: Option<String>,
    pub download_url: String,
    pub file_name: String,
}

/// Supported external downloader types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DownloaderType {
    #[serde(alias = "Aria2")]
    Aria2,
    #[serde(alias = "Motrix")]
    Motrix,
    #[serde(alias = "Idm")]
    Idm,
}

/// Information about an available application update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub changelog: String,
    pub download_url: String,
}

// Re-export CloudConfig from auth module for convenience.
// CloudConfig lives in auth::cloud_config because it is tightly coupled
// with CloudEnvironment and its config() method.
pub use crate::auth::cloud_config::CloudConfig;

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            language: String::from("system"),
            window: WindowState::default(),
            last_download_path: None,
        }
    }
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            x: -1, // -1 signals "center on primary display"
            y: -1,
            width: 1280,
            height: 720,
            is_maximized: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_entry_tolerates_missing_account_type() {
        // Legacy accounts.json entries predate the accountType field.
        let legacy = r#"{
            "homeAccountId": "user-1",
            "driveId": "drive-1",
            "cloudType": "global",
            "displayName": "Test User"
        }"#;
        let entry: AccountEntry = serde_json::from_str(legacy).unwrap();
        assert_eq!(entry.account_type, None);

        let entry = AccountEntry {
            home_account_id: "user-1".to_string(),
            drive_id: "drive-1".to_string(),
            cloud_type: CloudEnvironment::Global,
            display_name: "Test User".to_string(),
            account_type: Some(AccountCategory::Personal),
        };
        let value: serde_json::Value = serde_json::to_value(&entry).unwrap();
        assert_eq!(value["accountType"], "personal");
    }

    #[test]
    fn drive_item_serializes_with_camel_case_keys() {
        let item = DriveItem {
            id: "1".to_string(),
            name: "folder".to_string(),
            size: None,
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            is_folder: true,
            mime_type: None,
            web_url: Some("https://example.com".to_string()),
            parent_reference: Some(ParentReference {
                drive_id: "drive".to_string(),
                id: "parent".to_string(),
                path: None,
                name: None,
            }),
            download_url: None,
            created_date_time: None,
        };

        let value = serde_json::to_value(item).unwrap();
        assert_eq!(value["isFolder"], true);
        assert_eq!(value["lastModified"], "2026-01-01T00:00:00Z");
        assert!(value.get("webUrl").is_some());
        assert!(value.get("parentReference").is_some());
        assert_eq!(value["parentReference"]["driveId"], "drive");
    }

    #[test]
    fn account_entry_reads_legacy_snake_case_and_writes_camel_case() {
        let legacy = r#"{
            "home_account_id": "home",
            "drive_id": "drive",
            "cloud_type": "global",
            "display_name": "User"
        }"#;
        let entry: AccountEntry = serde_json::from_str(legacy).unwrap();
        assert_eq!(entry.cloud_type, CloudEnvironment::Global);

        let value = serde_json::to_value(entry).unwrap();
        assert_eq!(value["homeAccountId"], "home");
        assert_eq!(value["driveId"], "drive");
        assert_eq!(value["displayName"], "User");
    }

    #[test]
    fn config_reads_legacy_keys_and_serializes_camel_case() {
        let legacy = r#"{
            "theme": "System",
            "language": "system",
            "window": {
                "x": 1,
                "y": 2,
                "width": 1280,
                "height": 720,
                "is_maximized": true
            },
            "last_download_path": "C:\\Downloads"
        }"#;
        let config: AppConfig = serde_json::from_str(legacy).unwrap();
        assert_eq!(config.theme, ThemeMode::System);
        assert_eq!(config.last_download_path.as_deref(), Some("C:\\Downloads"));

        let value = serde_json::to_value(config).unwrap();
        assert_eq!(value["window"]["isMaximized"], true);
        assert_eq!(value["lastDownloadPath"], "C:\\Downloads");
    }

    #[test]
    fn meeting_recording_serializes_camel_case_and_source_lowercase() {
        let recording = MeetingRecording {
            drive_id: "drive".to_string(),
            item: DriveItem {
                id: "42".to_string(),
                name: "2026-08-20 14-30 - Sprint.mp4".to_string(),
                size: Some(1024),
                last_modified: "2026-08-20T06:30:00Z".to_string(),
                is_folder: false,
                mime_type: Some("video/mp4".to_string()),
                web_url: None,
                parent_reference: None,
                download_url: None,
                created_date_time: Some("2026-08-20T06:30:00Z".to_string()),
            },
            source_type: RecordingSource::SharePoint,
            source_name: "Engineering".to_string(),
        };

        let value = serde_json::to_value(recording).unwrap();
        assert_eq!(value["driveId"], "drive");
        assert_eq!(value["item"]["isFolder"], false);
        assert_eq!(value["item"]["createdDateTime"], "2026-08-20T06:30:00Z");
        assert_eq!(value["sourceType"], "sharepoint");
        assert_eq!(value["sourceName"], "Engineering");
    }

    #[test]
    fn share_options_reads_camel_case() {
        let options: ShareOptions = serde_json::from_value(serde_json::json!({
            "linkType": "edit",
            "expiration": null,
            "password": "secret"
        }))
        .unwrap();
        assert_eq!(options.link_type, "edit");
        assert_eq!(options.password.as_deref(), Some("secret"));
    }
}
