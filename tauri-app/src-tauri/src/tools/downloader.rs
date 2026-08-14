use crate::errors::AppError;
use crate::models::{DownloaderType, ExternalDownloaderConfig};
use serde_json::json;

/// Pushes a download URL to the configured external downloader.
///
/// Dispatches to the correct handler based on `config.downloader_type`:
/// - Aria2 and Motrix both use JSON-RPC 2.0
/// - IDM uses command-line invocation
pub async fn push_to_downloader(config: ExternalDownloaderConfig) -> Result<(), AppError> {
    match config.downloader_type {
        DownloaderType::Aria2 | DownloaderType::Motrix => push_to_aria2(&config).await,
        DownloaderType::Idm => push_to_idm(&config),
    }
}

/// Sends a JSON-RPC 2.0 request to Aria2 or Motrix to add a download URI.
///
/// Request format:
/// ```json
/// {
///   "jsonrpc": "2.0",
///   "id": "1",
///   "method": "aria2.addUri",
///   "params": ["token:<secret>", ["<download_url>"], {"out": "<file_name>"}]
/// }
/// ```
///
/// If `config.secret` is Some, includes `"token:{secret}"` as the first param.
async fn push_to_aria2(config: &ExternalDownloaderConfig) -> Result<(), AppError> {
    let mut params = Vec::new();

    // Add token if secret is provided
    if let Some(ref secret) = config.secret {
        if !secret.is_empty() {
            params.push(json!(format!("token:{}", secret)));
        }
    }

    // Add URI list
    params.push(json!([&config.download_url]));

    // Add options with output filename
    params.push(json!({"out": &config.file_name}));

    let body = json!({
        "jsonrpc": "2.0",
        "id": "1",
        "method": "aria2.addUri",
        "params": params
    });

    let client = reqwest::Client::new();
    let response = client
        .post(&config.rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Network {
            message: format!("Failed to connect to RPC endpoint: {}", e),
            retryable: true,
        })?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body_text = response.text().await.unwrap_or_default();
        return Err(AppError::Network {
            message: format!("RPC request failed (HTTP {}): {}", status, body_text),
            retryable: false,
        });
    }

    // Check for JSON-RPC error in response
    let resp_body: serde_json::Value = response.json().await.map_err(|e| AppError::Network {
        message: format!("Failed to parse RPC response: {}", e),
        retryable: false,
    })?;

    if let Some(error) = resp_body.get("error") {
        let error_msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown RPC error");
        return Err(AppError::Network {
            message: format!("RPC error: {}", error_msg),
            retryable: false,
        });
    }

    Ok(())
}

/// Invokes IDM (Internet Download Manager) via command line.
///
/// Command: `IDMan.exe /d <url> /f <filename> /n`
/// - `/d` specifies the download URL
/// - `/f` specifies the local filename
/// - `/n` starts download without showing confirmation dialog
fn push_to_idm(config: &ExternalDownloaderConfig) -> Result<(), AppError> {
    // The rpc_url field is repurposed as IDM executable path for IDM type
    let idm_path = if config.rpc_url.is_empty() {
        "IDMan.exe".to_string()
    } else {
        config.rpc_url.clone()
    };

    std::process::Command::new(&idm_path)
        .args(["/d", &config.download_url, "/f", &config.file_name, "/n"])
        .spawn()
        .map_err(|e| AppError::FileSystem {
            message: format!("Failed to start IDM: {}", e),
            path: idm_path,
        })?;

    Ok(())
}
