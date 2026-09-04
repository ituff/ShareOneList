use std::io::Write;
use std::time::Duration;

use crate::errors::AppError;
use crate::models::UpdateInfo;
use reqwest::Client;
use serde::Deserialize;
use tauri::Emitter;

const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/ituff/ShareOneList/releases/latest";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Download source prefixes tried in order. The empty prefix is GitHub direct;
/// the others are China-friendly GitHub acceleration mirrors. Prefixes must
/// simply be prepended to the full release download URL.
const DOWNLOAD_SOURCE_PREFIXES: &[&str] = &[
    "",
    "https://ghfast.top/",
    "https://gh-proxy.com/",
    "https://github.moeyy.xyz/",
];

/// Progress event emitted while the update installer downloads.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateDownloadProgress {
    transferred: u64,
    total: u64,
}

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

/// Architecture fragment used to identify this platform's assets.
fn platform_arch_fragment() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        "arm64"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "x64"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "aarch64"
    }
    // Fallback for other platforms.
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
    )))]
    {
        "x64"
    }
}

/// Installer extensions preferred for the current platform, in order of priority.
fn platform_installer_extensions() -> Vec<&'static str> {
    #[cfg(target_os = "windows")]
    {
        vec![".msi", ".exe"]
    }
    #[cfg(target_os = "macos")]
    {
        vec![".dmg", ".app"]
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        vec![]
    }
}

/// Pick the release asset best suited for this platform.
fn select_platform_asset<'a>(assets: &'a [GitHubAsset]) -> Option<&'a GitHubAsset> {
    let arch = platform_arch_fragment();
    let installers = platform_installer_extensions();
    select_platform_asset_with_preferences(assets, arch, &installers)
}

fn select_platform_asset_with_preferences<'a>(
    assets: &'a [GitHubAsset],
    arch_fragment: &str,
    installer_extensions: &[&str],
) -> Option<&'a GitHubAsset> {
    let lower_names: Vec<String> = assets
        .iter()
        .map(|a| a.name.to_lowercase())
        .collect();

    // Prefer an installer for this exact architecture.
    for extension in installer_extensions {
        if let Some(index) = lower_names.iter().position(|name| {
            name.contains(arch_fragment) && name.ends_with(extension)
        }) {
            return assets.get(index);
        }
    }

    // Fall back to an installer without an architecture match, then to any
    // architecture-specific asset (e.g. a portable zip).
    for extension in installer_extensions {
        if let Some(index) = lower_names
            .iter()
            .position(|name| name.ends_with(extension))
        {
            return assets.get(index);
        }
    }

    lower_names
        .iter()
        .position(|name| name.contains(arch_fragment))
        .and_then(|index| assets.get(index))
}

/// Check GitHub releases for a newer version.
/// Returns `Some(UpdateInfo)` if a newer release exists, `None` if up to date.
pub async fn check_update() -> Result<Option<UpdateInfo>, AppError> {
    let client = Client::new();

    let release = fetch_latest_release(&client).await?;

    let remote_version = release.tag_name.trim_start_matches('v').to_string();

    if !is_newer(&remote_version, CURRENT_VERSION) {
        return Ok(None);
    }

    // Find platform-specific download URL
    let download_url = select_platform_asset(&release.assets)
        .map(|a| a.browser_download_url.clone())
        .unwrap_or_default();

    Ok(Some(UpdateInfo {
        version: remote_version,
        changelog: release.body.unwrap_or_default(),
        download_url,
    }))
}

/// Fetch the latest release metadata from GitHub.
async fn fetch_latest_release(client: &Client) -> Result<GitHubRelease, AppError> {
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

    response.json::<GitHubRelease>().await.map_err(|e| {
        AppError::Network {
            message: format!("Failed to parse release data: {}", e),
            retryable: true,
        }
    })
}

/// HEAD-probe a download source; mirrors are often down, so dead sources are
/// skipped quickly instead of timing out during the real download.
async fn source_reachable(client: &Client, url: &str) -> bool {
    matches!(
        client.head(url).send().await,
        Ok(response) if response.status().is_success()
    )
}

/// Download the update asset for the given version and open the installer.
/// Download sources are tried in order (GitHub direct first, then China
/// acceleration mirrors); progress is emitted via the
/// `update-download-progress` event.
pub async fn perform_update(version: &str, app_handle: tauri::AppHandle) -> Result<(), AppError> {
    let client = Client::new();

    let release = fetch_latest_release(&client).await?;

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

    let asset = select_platform_asset(&release.assets).ok_or_else(|| AppError::Network {
        message: "No matching asset found for this platform".to_string(),
        retryable: false,
    })?;

    // Build the ordered source list: direct GitHub first, then mirrors.
    let sources: Vec<String> = DOWNLOAD_SOURCE_PREFIXES
        .iter()
        .map(|prefix| format!("{}{}", prefix, asset.browser_download_url))
        .collect();

    // A client with a short timeout only for probing sources.
    let probe_client = Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| AppError::Network {
            message: format!("Failed to build HTTP client: {}", e),
            retryable: true,
        })?;
    // A generous timeout for the actual (potentially large) download.
    let download_client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(1800))
        .build()
        .map_err(|e| AppError::Network {
            message: format!("Failed to build HTTP client: {}", e),
            retryable: true,
        })?;

    // Try each reachable source in order until one starts successfully.
    let mut response = None;
    let mut last_error: Option<AppError> = None;
    for url in &sources {
        if !source_reachable(&probe_client, url).await {
            eprintln!("[updater] source unreachable, skipping: {}", url);
            continue;
        }
        match download_client
            .get(url)
            .header("User-Agent", "ShareOneList-Updater")
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                response = Some(resp);
                break;
            }
            Ok(resp) => {
                last_error = Some(AppError::Network {
                    message: format!("Download returned status {}", resp.status()),
                    retryable: true,
                });
            }
            Err(e) => {
                last_error = Some(AppError::Network {
                    message: format!("Download failed: {}", e),
                    retryable: true,
                });
            }
        }
    }

    let mut response = response.ok_or_else(|| {
        last_error.unwrap_or(AppError::Network {
            message: "No download source available".to_string(),
            retryable: false,
        })
    })?;

    let total = response.content_length().unwrap_or(0);

    // Save to temp directory, streaming chunks and emitting progress.
    let temp_dir = std::env::temp_dir();
    let file_path = temp_dir.join(&asset.name);
    let mut file = std::fs::File::create(&file_path).map_err(|e| AppError::FileSystem {
        message: format!("Failed to save update file: {}", e),
        path: file_path.display().to_string(),
    })?;

    let mut transferred: u64 = 0;
    let mut last_emitted: u64 = 0;
    const PROGRESS_STEP: u64 = 512 * 1024;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| AppError::Network {
            message: format!("Download failed: {}", e),
            retryable: true,
        })?
    {
        file.write_all(&chunk).map_err(|e| AppError::FileSystem {
            message: format!("Failed to save update file: {}", e),
            path: file_path.display().to_string(),
        })?;
        transferred += chunk.len() as u64;
        if transferred - last_emitted >= PROGRESS_STEP {
            last_emitted = transferred;
            let _ = app_handle.emit(
                "update-download-progress",
                UpdateDownloadProgress {
                    transferred,
                    total,
                },
            );
        }
    }
    file.flush().ok();
    drop(file);

    let _ = app_handle.emit(
        "update-download-progress",
        UpdateDownloadProgress {
            transferred,
            total,
        },
    );

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

    fn asset(name: &str) -> GitHubAsset {
        GitHubAsset {
            name: name.to_string(),
            browser_download_url: format!("https://example.com/{}", name),
        }
    }

    #[test]
    fn test_select_windows_installer_over_zip() {
        let assets = vec![
            asset("ShareOneList-v2.0.0-x64.zip"),
            asset("ShareOneList_2.0.0_arm64_en-US.msi"),
            asset("ShareOneList_2.0.0_x64_en-US.msi"),
        ];
        let selected = select_platform_asset_with_preferences(&assets, "x64", &[".msi", ".exe"])
            .expect("asset should be selected");
        assert_eq!(selected.name, "ShareOneList_2.0.0_x64_en-US.msi");
    }

    #[test]
    fn test_select_macos_dmg_over_zip() {
        let assets = vec![
            asset("ShareOneList_2.0.0_aarch64.dmg"),
            asset("ShareOneList-v2.0.0-macos.zip"),
        ];
        let selected = select_platform_asset_with_preferences(&assets, "aarch64", &[".dmg", ".app"])
            .expect("asset should be selected");
        assert!(selected.name.ends_with(".dmg"));
    }

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
