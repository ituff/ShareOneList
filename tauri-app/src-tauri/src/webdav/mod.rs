// WebDAV gateway module: exposes OneDrive / SharePoint libraries as a
// loopback WebDAV server that Explorer / Finder can mount natively.
//
// Layout: server.rs (listener + auth + routing), fs.rs (Graph translation),
// cache.rs (metadata cache), mounts.rs (persistence), platform.rs (OS mount
// helpers + Windows diagnostics), commands.rs (Tauri commands).

pub mod cache;
pub mod commands;
pub mod fs;
pub mod mounts;
pub mod platform;
pub mod server;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use dav_server::DavHandler;

use crate::webdav::fs::GraphDavFs;
use crate::webdav::mounts::{MountConfig, MountEntry, MountStore, GATEWAY_USERNAME};
use crate::webdav::server::{MountMap, MountRuntime, ServerShared};

/// Sharable handle around [`WebDavManagerInner`]; managed as Tauri state and
/// cloned into the server's accept loop.
#[derive(Clone)]
pub struct WebDavManager {
    inner: Arc<WebDavManagerInner>,
}

struct WebDavManagerInner {
    app: tauri::AppHandle,
    store: MountStore,
    state: tokio::sync::RwLock<ManagerState>,
}

struct ManagerState {
    config: MountConfig,
    running: bool,
    port: u16,
    mounts: MountMap,
}

/// Status payload for the settings UI.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountStatus {
    #[serde(flatten)]
    pub entry: MountEntry,
    /// Full WebDAV URL of the mount (gateway base + /m/{id}).
    pub url: String,
    /// Best-effort OS-level mount detection (letters on Windows, /Volumes scan
    /// on macOS); `false` does not rule out a manual mount we cannot see.
    pub mounted: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavStatus {
    pub running: bool,
    pub port: u16,
    pub base_url: String,
    pub mounts: Vec<MountStatus>,
}

/// Credentials + URL for third-party WebDAV clients (RaiDrive etc.) when the
/// OS client is unavailable (e.g. Windows Home).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountConnectionInfo {
    pub url: String,
    pub username: String,
    pub password: String,
}

impl WebDavManager {
    pub fn new(app: tauri::AppHandle, data_dir: &std::path::Path) -> Self {
        Self {
            inner: Arc::new(WebDavManagerInner {
                app,
                store: MountStore::new(data_dir),
                state: tokio::sync::RwLock::new(ManagerState {
                    config: MountConfig::default(),
                    running: false,
                    port: 0,
                    mounts: Arc::new(RwLock::new(HashMap::new())),
                }),
            }),
        }
    }

    pub fn app_handle(&self) -> &tauri::AppHandle {
        &self.inner.app
    }

    /// Start the loopback listener and register all persisted mounts.
    /// Idempotent: returns the bound port when already running. The bound
    /// port is written back to the persisted config so gateway URLs stay
    /// stable across restarts (OS remembered credentials keep working).
    pub async fn start(&self) -> Result<u16, String> {
        let mut state = self.inner.state.write().await;
        if state.running {
            return Ok(state.port);
        }

        // Load persisted config fresh (single writer; manager owns the file).
        state.config = self.inner.store.load();

        let (listener, port) =
            server::bind_listener(state.config.port)
                .await
                .map_err(|e| format!("failed to bind WebDAV listener: {}", e))?;
        if state.config.port != port {
            state.config.port = port;
        }
        self.inner.store.save(&state.config)?;

        for entry in state.config.mounts.clone() {
            let runtime = self.build_runtime(&entry);
            state
                .mounts
                .write()
                .expect("mount map poisoned")
                .insert(entry.mount_id.clone(), runtime);
        }

        let shared = ServerShared {
            mounts: Arc::clone(&state.mounts),
            password: state.config.gateway_password.clone(),
            port,
        };
        tauri::async_runtime::spawn(server::accept_loop(listener, shared));

        state.running = true;
        state.port = port;
        Ok(port)
    }

    fn build_runtime(&self, entry: &MountEntry) -> MountRuntime {
        let fs = GraphDavFs::new(
            self.inner.app.clone(),
            entry.cloud_env.clone(),
            entry.home_account_id.clone(),
            entry.drive_id.clone(),
            entry.root_item_id.clone(),
        );
        let handler = DavHandler::builder()
            .filesystem(Box::new(fs))
            // Fake locksystem advertises DAV class 2 (Office needs it); P1
            // swaps in real write support behind the same locksystem.
            .locksystem(dav_server::fakels::FakeLs::new())
            .strip_prefix(format!("/m/{}", entry.mount_id))
            .autoindex(false)
            .hide_symlinks(true)
            .read_buf_size(256 * 1024)
            .principal(GATEWAY_USERNAME)
            .build_handler();
        MountRuntime {
            entry: entry.clone(),
            handler,
        }
    }

    fn mount_url(port: u16, mount_id: &str) -> String {
        format!("http://127.0.0.1:{}/m/{}", port, mount_id)
    }

    pub async fn create_mount(&self, entry: MountEntry) -> Result<MountEntry, String> {
        let mut state = self.inner.state.write().await;
        if state
            .config
            .mounts
            .iter()
            .any(|m| {
                m.cloud_env == entry.cloud_env
                    && m.home_account_id == entry.home_account_id
                    && m.drive_id == entry.drive_id
                    && m.root_item_id == entry.root_item_id
            })
        {
            return Err("duplicate_mount".to_string());
        }
        state
            .mounts
            .write()
            .expect("mount map poisoned")
            .insert(entry.mount_id.clone(), self.build_runtime(&entry));
        state.config.mounts.push(entry.clone());
        self.inner.store.save(&state.config)?;
        Ok(entry)
    }

    pub async fn delete_mount(&self, mount_id: &str) -> Result<(), String> {
        let mut state = self.inner.state.write().await;
        let before = state.config.mounts.len();
        state.config.mounts.retain(|m| m.mount_id != mount_id);
        if state.config.mounts.len() == before {
            return Err("mount_not_found".to_string());
        }
        state
            .mounts
            .write()
            .expect("mount map poisoned")
            .remove(mount_id);
        self.inner.store.save(&state.config)?;
        Ok(())
    }

    pub async fn set_drive_letter(&self, mount_id: &str, letter: Option<String>) -> Result<(), String> {
        let mut state = self.inner.state.write().await;
        let Some(entry) = state
            .config
            .mounts
            .iter_mut()
            .find(|m| m.mount_id == mount_id)
        else {
            return Err("mount_not_found".to_string());
        };
        entry.drive_letter = letter;
        self.inner.store.save(&state.config)?;
        Ok(())
    }

    pub async fn status(&self) -> WebDavStatus {
        let state = self.inner.state.read().await;
        let base = if state.running {
            Some(state.port)
        } else {
            None
        };
        let mounts = state
            .config
            .mounts
            .iter()
            .map(|entry| {
                let url = base
                    .map(|p| Self::mount_url(p, &entry.mount_id))
                    .unwrap_or_default();
                let mounted = base
                    .map(|p| {
                        platform::detect_mounted(
                            p,
                            &entry.mount_id,
                            entry.drive_letter.as_deref(),
                        )
                    })
                    .unwrap_or(false);
                MountStatus {
                    entry: entry.clone(),
                    url,
                    mounted,
                }
            })
            .collect();
        WebDavStatus {
            running: state.running,
            port: state.port,
            base_url: base
                .map(|p| format!("http://127.0.0.1:{}", p))
                .unwrap_or_default(),
            mounts,
        }
    }

    /// Lookup one mount + gateway credentials (for OS mount and copy-out).
    pub async fn connection_info(&self, mount_id: &str) -> Result<(MountEntry, MountConnectionInfo, u16), String> {
        let state = self.inner.state.read().await;
        if !state.running {
            return Err("server_not_running".to_string());
        }
        let entry = state
            .config
            .mounts
            .iter()
            .find(|m| m.mount_id == mount_id)
            .cloned()
            .ok_or_else(|| "mount_not_found".to_string())?;
        let info = MountConnectionInfo {
            url: Self::mount_url(state.port, mount_id),
            username: GATEWAY_USERNAME.to_string(),
            password: state.config.gateway_password.clone(),
        };
        Ok((entry, info, state.port))
    }
}

/// Generate a fresh random mount id (URL-safe, 12 hex chars).
pub fn generate_mount_id() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 6];
    rand::thread_rng().fill(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
