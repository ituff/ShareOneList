// Tools module: external downloader integration, SharePoint URL parser, updater

pub mod downloader;
pub mod updater;
pub mod url_parser;

pub mod commands {
    use crate::errors::AppError;
    use crate::models::{ExternalDownloaderConfig, UpdateInfo};

    /// Parses a SharePoint sharing URL and returns a direct download link.
    ///
    /// Returns None for non-SharePoint URLs, folder links, or malformed URLs.
    #[tauri::command]
    pub fn parse_sharepoint_url(url: String) -> Option<String> {
        super::url_parser::parse_sharepoint_url(&url)
    }

    /// Pushes a download URL to an external downloader (Aria2, Motrix, or IDM).
    #[tauri::command]
    pub async fn push_to_downloader(config: ExternalDownloaderConfig) -> Result<(), AppError> {
        super::downloader::push_to_downloader(config).await
    }

    /// Check GitHub releases for an available update.
    /// Returns Some(UpdateInfo) if a newer version is available, None if up to date.
    #[tauri::command]
    pub async fn check_update() -> Result<Option<UpdateInfo>, AppError> {
        super::updater::check_update().await
    }

    /// Download and open the installer/archive for the specified version.
    #[tauri::command]
    pub async fn perform_update(version: String) -> Result<(), AppError> {
        super::updater::perform_update(&version).await
    }
}
