use crate::errors::AppError;
use crate::models::UpdateInfo;
use reqwest::Client;
use serde::Deserialize;

const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/ituff/SimpleList21V/releases/latest";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    body: Option<String>,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// Compare two semver-style version strings (e.g. "2.0.0" vs "2.1.0").
/// Returns true if `remote` is newer than `local`.
fn is_newer(remote: &str, local: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse::<u64>().ok())
            .collect()
    };

    let r = parse(remote);
    let l = parse(local);

    for i in 0..r.len().max(l.len()) {
        let rv = r.get(i).copied().unwrap_or(0);
        let lv = l.get(i).copied().unwrap_or(0);
        if rv > lv {
            return true;
        }
        if rv < lv {
            return false;
        }
    }
    false
}

/// Determine the platform-specific asset name fragment to match.
fn platform_asset_filter() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "x64"
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        "arm64"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "macos"
    }
    // Fallback for other platforms (linux x86_64, etc.)
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "aarch64"),
    )))]
    {
        "x64"
    }
}

/// Check GitHub releases for a newer version.
/// Returns `Some(UpdateInfo)` if a newer release exists, `None` if up to date.
pub async fn check_update() -> Result<Option<UpdateInfo>, AppError> {
    let client = Client::new();

    let response = client
        .get(GITHUB_RELEASES_URL)
        .header("User-Agent", "ShareOneList-Updater")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AppError::Network {
            message: format!("Failed to reach GitHub: {}", e),
            retryable: true,
        })?;

    if !response.status().is_success() {
        return Err(AppError::Network {
            message: format!("GitHub API returned status {}", response.status()),
            retryable: true,
        });
    }

    let release: GitHubRelease =
        response.json().await.map_err(|e| AppError::Network {
            message: format!("Failed to parse release data: {}", e),
            retryable: true,
        })?;

    let remote_version = release.tag_name.trim_start_matches('v').to_string();

    if !is_newer(&remote_version, CURRENT_VERSION) {
        return Ok(None);
    }

    // Find platform-specific download URL
    let filter = platform_asset_filter();
    let download_url = release
        .assets
        .iter()
        .find(|a| a.name.to_lowercase().contains(filter))
        .map(|a| a.browser_download_url.clone())
        .unwrap_or_default();

    Ok(Some(UpdateInfo {
        version: remote_version,
        changelog: release.body.unwrap_or_default(),
        download_url,
    }))
}

/// Download the update asset for the given version and open the installer.
pub async fn perform_update(version: &str) -> Result<(), AppError> {
    let client = Client::new();

    // Fetch release info again to get the download URL for the requested version
    let response = client
        .get(GITHUB_RELEASES_URL)
        .header("User-Agent", "ShareOneList-Updater")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AppError::Network {
            message: format!("Failed to reach GitHub: {}", e),
            retryable: true,
        })?;

    if !response.status().is_success() {
        return Err(AppError::Network {
            message: format!("GitHub API returned status {}", response.status()),
            retryable: true,
        });
    }

    let release: GitHubRelease =
        response.json().await.map_err(|e| AppError::Network {
            message: format!("Failed to parse release data: {}", e),
            retryable: true,
        })?;

    let release_version = release.tag_name.trim_start_matches('v');
    if release_version != version.trim_start_matches('v') {
        return Err(AppError::Network {
            message: format!(
                "Version mismatch: expected {}, got {}",
                version, release_version
            ),
            retryable: true,
        });
    }

    let filter = platform_asset_filter();
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.to_lowercase().contains(filter))
        .ok_or_else(|| AppError::Network {
            message: "No matching asset found for this platform".to_string(),
            retryable: false,
        })?;

    // Download the asset to a temp file
    let download_response = client
        .get(&asset.browser_download_url)
        .header("User-Agent", "ShareOneList-Updater")
        .send()
        .await
        .map_err(|e| AppError::Network {
            message: format!("Download failed: {}", e),
            retryable: true,
        })?;

    if !download_response.status().is_success() {
        return Err(AppError::Network {
            message: format!(
                "Download returned status {}",
                download_response.status()
            ),
            retryable: true,
        });
    }

    let bytes = download_response.bytes().await.map_err(|e| AppError::Network {
        message: format!("Failed to read download: {}", e),
        retryable: true,
    })?;

    // Save to temp directory
    let temp_dir = std::env::temp_dir();
    let file_path = temp_dir.join(&asset.name);

    std::fs::write(&file_path, &bytes).map_err(|e| AppError::FileSystem {
        message: format!("Failed to save update file: {}", e),
        path: file_path.display().to_string(),
    })?;

    // Open the downloaded file (runs installer / opens archive)
    open::that(&file_path).map_err(|e| AppError::FileSystem {
        message: format!("Failed to open update file: {}", e),
        path: file_path.display().to_string(),
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer() {
        assert!(is_newer("2.1.0", "2.0.0"));
        assert!(is_newer("3.0.0", "2.9.9"));
        assert!(is_newer("2.0.1", "2.0.0"));
        assert!(!is_newer("2.0.0", "2.0.0"));
        assert!(!is_newer("1.9.9", "2.0.0"));
        assert!(is_newer("v2.1.0", "2.0.0"));
        assert!(is_newer("2.1.0", "v2.0.0"));
    }
}
