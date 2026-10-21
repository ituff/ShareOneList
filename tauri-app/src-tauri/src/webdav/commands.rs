// Tauri commands for the WebDAV gateway (status / mount management /
// OS mount / Windows diagnostics). Frontend wrappers live in
// src/lib/tauri.ts; every new command must also be registered in lib.rs.

use serde::Serialize;
use tauri::State;
use tokio::sync::Mutex;

use crate::auth::cloud_config::CloudEnvironment;
use crate::auth::AuthModule;
use crate::errors::AppError;
use crate::webdav::fs::GraphDavFs;
use crate::webdav::mounts::{validate_mount_id, MountEntry};
use crate::webdav::{
    generate_mount_id, platform, WebDavManager, WebDavStatus,
};

fn config_err(message: String) -> AppError {
    AppError::Config { message }
}

fn parse_env(cloud_env: &str) -> Result<CloudEnvironment, AppError> {
    match cloud_env.to_lowercase().as_str() {
        "global" => Ok(CloudEnvironment::Global),
        "china" => Ok(CloudEnvironment::China),
        _ => Err(AppError::Validation {
            message: format!("Invalid cloud environment '{}'.", cloud_env),
            field: "cloudEnv".to_string(),
        }),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountResult {
    /// "X:" on Windows, the webdav:// URL on macOS.
    pub mount_point: String,
}

#[tauri::command]
pub async fn webdav_status(manager: State<'_, WebDavManager>) -> Result<WebDavStatus, AppError> {
    Ok(manager.status().await)
}

#[tauri::command]
pub async fn webdav_create_mount(
    cloud_env: String,
    home_account_id: String,
    drive_id: String,
    root_path: String,
    label: String,
    drive_letter: Option<String>,
    // Custom URL path segment; absent/empty = random 12-hex id.
    mount_id: Option<String>,
    manager: State<'_, WebDavManager>,
    auth_module: State<'_, Mutex<AuthModule>>,
) -> Result<MountEntry, AppError> {
    let env = parse_env(&cloud_env)?;

    // Empty / absent → random id; a custom id must be a URL-safe path
    // segment (uniqueness is enforced by the manager against all mounts).
    let mount_id = match mount_id.as_deref().map(str::trim) {
        Some(id) if !id.is_empty() => {
            validate_mount_id(id).map_err(|_| AppError::Validation {
                message: "invalid_mount_id".to_string(),
                field: "mountId".to_string(),
            })?;
            id.to_string()
        }
        _ => generate_mount_id(),
    };

    // Fail early when the account has no live session (mounts are useless
    // without one, and the error must surface as an auth re-login prompt).
    {
        let mut auth = auth_module.lock().await;
        auth.get_token_for_account(env.clone(), &home_account_id)
            .await?;
    }

    let mut root_item_id = String::new();
    if !root_path.trim().is_empty() {
        let fs = GraphDavFs::new(
            manager.app_handle().clone(),
            env.clone(),
            home_account_id.clone(),
            drive_id.clone(),
            String::new(),
        );
        let item = fs
            .resolve_root(&root_path)
            .await
            .map_err(|_| AppError::GraphApi {
                message: format!("mount root path not found: {}", root_path),
                status_code: 404,
            })?;
        if !item.is_dir {
            return Err(AppError::Validation {
                message: format!("mount root path is not a folder: {}", root_path),
                field: "rootPath".to_string(),
            });
        }
        root_item_id = item.id;
    }

    let entry = MountEntry {
        mount_id,
        cloud_env: env,
        home_account_id,
        drive_id,
        root_item_id,
        label,
        drive_letter,
        // Not yet mounted; `WebDavManager::mount` sets the flag when the user
        // maps the drive.
        auto_mount: false,
    };
    manager
        .create_mount(entry.clone())
        .await
        .map_err(config_err)?;
    Ok(entry)
}

#[tauri::command]
pub async fn webdav_delete_mount(
    mount_id: String,
    manager: State<'_, WebDavManager>,
) -> Result<(), AppError> {
    manager.delete_mount(&mount_id).await.map_err(config_err)
}

/// Change a mount's URL path segment (`/m/{id}`). Existing OS mappings keep
/// pointing at the old URL and must be unmounted + re-mounted afterwards.
#[tauri::command]
pub async fn webdav_rename_mount(
    mount_id: String,
    new_mount_id: String,
    manager: State<'_, WebDavManager>,
) -> Result<MountEntry, AppError> {
    manager
        .rename_mount(&mount_id, &new_mount_id)
        .await
        .map_err(config_err)
}

/// Map the mount in the OS (drive letter on Windows, Finder volume on macOS)
/// and flag it for auto-restore on the next app start.
#[tauri::command]
pub async fn webdav_mount(
    mount_id: String,
    manager: State<'_, WebDavManager>,
) -> Result<MountResult, AppError> {
    let point = manager.mount(&mount_id).await.map_err(config_err)?;
    Ok(MountResult { mount_point: point })
}

/// Unmap the mount and clear its auto-restore flag (explicit unmount only).
#[tauri::command]
pub async fn webdav_unmount(
    mount_id: String,
    manager: State<'_, WebDavManager>,
) -> Result<(), AppError> {
    manager.unmount(&mount_id).await.map_err(config_err)
}

#[tauri::command]
pub async fn webdav_diagnose() -> Result<platform::Diagnosis, AppError> {
    Ok(platform::diagnose())
}

#[tauri::command]
pub async fn webdav_apply_fix() -> Result<(), AppError> {
    platform::apply_fix().map_err(config_err)
}

/// URL + gateway credentials for third-party WebDAV clients (Windows Home).
#[tauri::command]
pub async fn webdav_copy_mount_info(
    mount_id: String,
    manager: State<'_, WebDavManager>,
) -> Result<crate::webdav::MountConnectionInfo, AppError> {
    let (_entry, info, _port) = manager
        .connection_info(&mount_id)
        .await
        .map_err(config_err)?;
    Ok(info)
}
