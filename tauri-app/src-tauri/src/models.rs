use serde::{Deserialize, Serialize};

use crate::auth::cloud_config::CloudEnvironment;

/// Drive item representing a file or folder in OneDrive/SharePoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct ParentReference {
    pub drive_id: String,
    pub id: String,
    pub path: Option<String>,
    pub name: Option<String>,
}

/// Represents a drive (OneDrive personal, OneDrive for Business, or SharePoint document library).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drive {
    pub id: String,
    pub name: String,
    pub drive_type: String,
    pub quota: Option<DriveQuota>,
}

/// Storage quota information for a drive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveQuota {
    pub total: u64,
    pub used: u64,
    pub remaining: u64,
}

/// A SharePoint site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Site {
    pub id: String,
    pub display_name: String,
    pub web_url: String,
}

/// Transfer progress emitted via Tauri events.
#[derive(Debug, Clone, Serialize)]
pub struct ProgressEvent {
    pub task_id: String,
    pub file_name: String,
    pub status: String, // "downloading" | "uploading" | "paused" | "completed" | "failed"
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub speed_bps: u64,
    pub elapsed_secs: f64,
    pub error: Option<String>,
}

/// Application configuration persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub theme: ThemeMode,
    pub language: String,
    pub window: WindowState,
}

/// Theme mode preference.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

/// Window position and size state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_maximized: bool,
}

/// A persisted account entry linking a user to a drive and cloud environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountEntry {
    pub home_account_id: String,
    pub drive_id: String,
    pub cloud_type: CloudEnvironment,
    pub display_name: String,
}

/// Options for creating a share link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareOptions {
    pub link_type: String, // "view" | "edit"
    pub expiration: Option<String>, // ISO 8601 date
    pub password: Option<String>,
}

/// Configuration for pushing a download to an external downloader.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalDownloaderConfig {
    pub downloader_type: DownloaderType,
    pub rpc_url: String,
    pub secret: Option<String>,
    pub download_url: String,
    pub file_name: String,
}

/// Supported external downloader types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DownloaderType {
    Aria2,
    Motrix,
    Idm,
}

/// Information about an available application update.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
            language: String::from("en-US"),
            window: WindowState::default(),
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
