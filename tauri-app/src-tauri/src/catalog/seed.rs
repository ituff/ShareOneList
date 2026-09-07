// Root-level seed indexing and optional manual deep indexing. The seed is
// the only automatic indexing: one paginated root-children listing per
// drive. Deep indexing is a user-initiated BFS with cancellation support.

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::auth::cloud_config::CloudEnvironment;
use crate::catalog::store::{CatalogNodeInput, CatalogStore};
use crate::errors::AppError;
use crate::graph::GraphClient;

/// Tauri event name for catalog progress updates.
pub const CATALOG_EVENT: &str = "catalog-event";

pub const DEFAULT_MAX_DEPTH: usize = 5;
pub const MAX_ALLOWED_DEPTH: usize = 10;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEvent {
    pub account_id: String,
    pub drive_id: String,
    /// 'seeding' | 'ready' | 'failed' | 'indexing' | 'cancelled'
    pub status: String,
    pub visited_nodes: i64,
    pub current_path: String,
}

/// One child entry parsed out of a Graph children listing.
#[derive(Debug, Clone)]
pub struct ChildEntry {
    pub item_id: String,
    pub name: String,
    pub path: String, // relative to drive root
    pub kind: String,
}

/// Converts Graph's `parentReference.path` ("/drive/root:/AE&TS/03") into a
/// drive-root-relative path ("AE&TS/03").
pub fn relative_root_path(parent_path: &str) -> String {
    match parent_path.split_once(":/") {
        Some((_, rest)) => rest.to_string(),
        None => String::new(),
    }
}

fn child_entry(value: &Value) -> Option<ChildEntry> {
    let item_id = value.get("id")?.as_str()?.to_string();
    let name = value.get("name")?.as_str()?.to_string();
    let parent = value
        .get("parentReference")
        .and_then(|p| p.get("path"))
        .and_then(|p| p.as_str())
        .unwrap_or_default();
    let base = relative_root_path(parent);
    let path = if base.is_empty() {
        name.clone()
    } else {
        format!("{base}/{name}")
    };
    let kind = if value.get("folder").is_some() {
        "folder"
    } else {
        "file"
    };
    Some(ChildEntry {
        item_id,
        name,
        path,
        kind: kind.into(),
    })
}

fn select_fields() -> &'static str {
    "$top=200&$select=id,name,folder,file,parentReference"
}

/// Lists one page of a folder's children. `folder` uses the Graph item id;
/// `None` means the drive root. Returns entries plus the next link if any.
async fn list_children_page(
    client: &GraphClient,
    token: &str,
    drive_id: &str,
    folder: Option<&str>,
    next_link: Option<&str>,
) -> Result<(Vec<ChildEntry>, Option<String>), AppError> {
    let url = match next_link {
        Some(link) => link.to_string(),
        None => match folder {
            Some(id) => format!(
                "{}/drives/{}/items/{}/children?{}",
                client.base_url(),
                drive_id,
                id,
                select_fields()
            ),
            None => format!(
                "{}/drives/{}/root/children?{}",
                client.base_url(),
                drive_id,
                select_fields()
            ),
        },
    };
    let response = client
        .request_with_retry(token, |http, tkn| http.get(&url).bearer_auth(tkn))
        .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::GraphApi {
            message: format!("catalog listing failed: HTTP {}", status.as_u16()),
            status_code: status.as_u16(),
        });
    }
    let json: Value = response.json().await.map_err(|e| AppError::Network {
        message: format!("catalog listing decode failed: {}", e),
        retryable: false,
    })?;
    let mut entries = Vec::new();
    if let Some(array) = json.get("value").and_then(|v| v.as_array()) {
        for item in array {
            if let Some(entry) = child_entry(item) {
                entries.push(entry);
            }
        }
    }
    let next = json
        .get("@odata.nextLink")
        .and_then(|v| v.as_str())
        .map(String::from);
    Ok((entries, next))
}

async fn list_children_all(
    client: &GraphClient,
    token: &str,
    drive_id: &str,
    folder: Option<&str>,
    cancel: Option<&CancellationToken>,
) -> Result<Vec<ChildEntry>, AppError> {
    let mut all = Vec::new();
    let mut next: Option<String> = None;
    loop {
        if let Some(cancel) = cancel {
            if cancel.is_cancelled() {
                return Ok(all);
            }
        }
        let (entries, next_link) =
            list_children_page(client, token, drive_id, folder, next.as_deref()).await?;
        all.extend(entries);
        match next_link {
            Some(link) => next = Some(link),
            None => break,
        }
    }
    Ok(all)
}

fn emit(
    app: &tauri::AppHandle,
    account_id: &str,
    drive_id: &str,
    status: &str,
    visited: i64,
    current_path: &str,
) {
    use tauri::Emitter;
    let _ = app.emit(
        CATALOG_EVENT,
        CatalogEvent {
            account_id: account_id.to_string(),
            drive_id: drive_id.to_string(),
            status: status.into(),
            visited_nodes: visited,
            current_path: current_path.into(),
        },
    );
}

/// Seeds the drive's root level: one paginated root-children listing. This
/// is the only automatic indexing (requirements 2.1).
pub async fn run_seed(
    store: &CatalogStore,
    app: &tauri::AppHandle,
    account_id: &str,
    env: CloudEnvironment,
    drive_id: &str,
    token: &str,
) -> Result<usize, AppError> {
    store.set_status(account_id, drive_id, "seeding")?;
    emit(app, account_id, drive_id, "seeding", 0, "");
    let client = GraphClient::new(env);
    let result = list_children_all(&client, token, drive_id, None, None).await;
    match result {
        Ok(entries) => {
            let writes: Vec<(CatalogNodeInput, i64)> = entries
                .iter()
                .map(|e| {
                    (
                        CatalogNodeInput {
                            path: e.path.clone(),
                            item_id: e.item_id.clone(),
                            name: e.name.clone(),
                            kind: e.kind.clone(),
                            desc: String::new(),
                        },
                        0,
                    )
                })
                .collect();
            let count = writes.len() as i64;
            store.upsert_nodes_mixed(account_id, drive_id, &writes)?;
            store.set_status(account_id, drive_id, "ready")?;
            emit(app, account_id, drive_id, "ready", count, "");
            Ok(count as usize)
        }
        Err(e) => {
            store.set_status(account_id, drive_id, "failed")?;
            emit(app, account_id, drive_id, "failed", 0, "");
            Err(e)
        }
    }
}

/// User-initiated depth-limited BFS index. Cancellation keeps everything
/// written so far (batches are committed per folder) and reports
/// 'cancelled' (requirements 2.5).
pub async fn run_deep_index(
    store: &CatalogStore,
    app: &tauri::AppHandle,
    account_id: &str,
    env: CloudEnvironment,
    drive_id: &str,
    token: &str,
    max_depth: usize,
    cancel: CancellationToken,
) -> Result<usize, AppError> {
    let max_depth = max_depth.clamp(1, MAX_ALLOWED_DEPTH);
    store.set_status(account_id, drive_id, "indexing")?;
    emit(app, account_id, drive_id, "indexing", 0, "");

    let client = GraphClient::new(env);
    let mut queue: std::collections::VecDeque<(String, String, usize)> =
        std::collections::VecDeque::new(); // (item_id, path, depth)
    queue.push_back(("root".to_string(), String::new(), 0));
    let mut visited: i64 = 0;
    let mut cancelled = false;

    while let Some((item_id, path, depth)) = queue.pop_front() {
        if cancel.is_cancelled() {
            cancelled = true;
            break;
        }
        let entries =
            list_children_all(&client, token, drive_id, Some(&item_id), Some(&cancel)).await?;
        let mut writes: Vec<(CatalogNodeInput, i64)> = Vec::new();
        for entry in entries {
            let entry_path = if path.is_empty() {
                entry.name.clone()
            } else {
                format!("{}/{}", path, entry.name)
            };
            visited += 1;
            if entry.kind == "folder" && depth + 1 < max_depth {
                queue.push_back((entry.item_id.clone(), entry_path.clone(), depth + 1));
            }
            writes.push((
                CatalogNodeInput {
                    path: entry_path.clone(),
                    item_id: entry.item_id.clone(),
                    name: entry.name.clone(),
                    kind: entry.kind.clone(),
                    desc: String::new(),
                },
                0,
            ));
        }
        // Committed per folder: a cancel leaves every finished folder intact.
        store.upsert_nodes_mixed(account_id, drive_id, &writes)?;
        emit(app, account_id, drive_id, "indexing", visited, &path);
    }

    let count = store.refresh_node_count(account_id, drive_id)?;
    let status = if cancelled { "cancelled" } else { "ready" };
    store.set_status(account_id, drive_id, status)?;
    emit(app, account_id, drive_id, status, count, "");
    Ok(count as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The path logic is the testable core of the listing pipeline.
    #[test]
    fn relative_root_path_strips_drive_prefix() {
        assert_eq!(
            relative_root_path("/drive/root:/AE&TS/03. TLOB"),
            "AE&TS/03. TLOB"
        );
        assert_eq!(relative_root_path("/drive/root:"), "");
        assert_eq!(relative_root_path(""), "");
    }

    #[test]
    fn child_entry_parses_graph_shape() {
        let value: Value = serde_json::from_str(
            r#"{
                "id": "01ABC",
                "name": "report.pdf",
                "parentReference": { "path": "/drive/root:/AE&TS" },
                "file": { "mimeType": "application/pdf" }
            }"#,
        )
        .unwrap();
        let entry = child_entry(&value).unwrap();
        assert_eq!(entry.item_id, "01ABC");
        assert_eq!(entry.name, "report.pdf");
        assert_eq!(entry.path, "AE&TS/report.pdf");
        assert_eq!(entry.kind, "file");
    }

    #[test]
    fn child_entry_detects_folders_and_root_entries() {
        let value: Value = serde_json::from_str(
            r#"{
                "id": "01F",
                "name": "Lab",
                "folder": { "childCount": 3 },
                "parentReference": { "path": "/drive/root:" }
            }"#,
        )
        .unwrap();
        let entry = child_entry(&value).unwrap();
        assert_eq!(entry.kind, "folder");
        assert_eq!(entry.path, "Lab", "root-level entries use bare name");
    }
}
