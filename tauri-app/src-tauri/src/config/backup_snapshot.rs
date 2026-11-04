// Backup snapshot assembly and restore: turns the base backup payload
// (config + accounts) into a full version-2 payload by collecting the
// user-selected optional sections and encrypting them, and restores those
// sections back into the app during import.

use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::auth::cloud_config::CloudEnvironment;
use crate::catalog::store::{CatalogSnapshot, CatalogStore};
use crate::llm::config::LlmConfigManager;
use crate::models::{AccountEntry, BackupItem};
use crate::store::chat_history::{
    export_chat_history_snapshot, import_chat_history_snapshot, ChatHistorySnapshot,
};
use crate::webdav::mounts::MountStore;

use super::backup_crypto;

/// Small state file tracking the fingerprint of the dynamic sections
/// (chat history / catalog) that were last uploaded. The startup and exit
/// backup triggers consult it to skip unchanged content.
const BACKUP_STATE_FILE_NAME: &str = "backup-state.json";

/// Default items used when the payload's config predates the field.
fn items_from_payload(payload: &Value) -> Vec<BackupItem> {
    payload["config"]["autoBackupItems"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value::<BackupItem>(v.clone()).ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// Collect the requested optional sections from live app state. Returns the
/// sections JSON plus the items that were actually collected — a section
/// whose collection fails is skipped (logged) rather than failing the whole
/// backup.
pub fn collect_sections(
    app: &AppHandle,
    items: &[BackupItem],
    accounts: &[AccountEntry],
) -> (Value, Vec<BackupItem>) {
    let mut sections = serde_json::Map::new();
    let mut collected = Vec::new();
    let data_dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("[config] backup: cannot resolve app data dir: {}", e);
            return (Value::Object(sections), collected);
        }
    };

    for item in items {
        let result: Option<Value> = match item {
            BackupItem::Tokens => {
                if accounts.is_empty() {
                    None
                } else {
                    let tokens: Vec<Value> = accounts
                        .iter()
                        .filter_map(|account| {
                            crate::auth::load_refresh_token(&account.cloud_type, &account.home_account_id)
                                .map(|token| {
                                    json!({
                                        "homeAccountId": account.home_account_id,
                                        "cloudEnv": account.cloud_type,
                                        "refreshToken": token,
                                    })
                                })
                        })
                        .collect();
                    Some(json!(tokens))
                }
            }
            BackupItem::LlmConfig => app
                .try_state::<LlmConfigManager>()
                .map(|manager| serde_json::to_value(manager.load()).ok())
                .and_then(|v| v),
            BackupItem::LlmKeys => {
                let keys: serde_json::Map<String, Value> = app
                    .try_state::<LlmConfigManager>()
                    .map(|manager| {
                        manager
                            .load()
                            .providers
                            .iter()
                            .filter_map(|provider| {
                                manager
                                    .load_api_key(&provider.id)
                                    .map(|key| (provider.id.clone(), Value::String(key)))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Some(Value::Object(keys))
            }
            BackupItem::ChatHistory => {
                let db_path = app
                    .try_state::<std::sync::Arc<crate::store::chat_history::ChatHistoryStore>>()
                    .map(|store| store.db_path().clone());
                match db_path {
                    Some(db_path) => match export_chat_history_snapshot(&db_path) {
                        Ok(snapshot) => serde_json::to_value(snapshot).ok(),
                        Err(e) => {
                            eprintln!("[config] backup: chat history snapshot failed: {}", e);
                            None
                        }
                    },
                    None => None,
                }
            }
            BackupItem::Catalog => app
                .try_state::<std::sync::Arc<CatalogStore>>()
                .and_then(|store| match store.export_all() {
                    Ok(snapshot) => serde_json::to_value(snapshot).ok(),
                    Err(e) => {
                        eprintln!("[config] backup: catalog snapshot failed: {}", e);
                        None
                    }
                }),
            BackupItem::WebdavMounts => serde_json::to_value(MountStore::new(&data_dir).load()).ok(),
            BackupItem::DownloadTasks => Some(collect_download_tasks(&data_dir)),
        };

        if let Some(section) = result {
            sections.insert(section_key(*item).to_string(), section);
            collected.push(*item);
        } else {
            eprintln!("[config] backup: section {:?} unavailable, skipped", item);
        }
    }

    (Value::Object(sections), collected)
}

/// JSON key of a section inside the (encrypted) sections map.
fn section_key(item: BackupItem) -> &'static str {
    match item {
        BackupItem::Tokens => "tokens",
        BackupItem::LlmConfig => "llmConfig",
        BackupItem::LlmKeys => "llmApiKeys",
        BackupItem::ChatHistory => "chatHistory",
        BackupItem::Catalog => "catalog",
        BackupItem::WebdavMounts => "webdavMounts",
        BackupItem::DownloadTasks => "downloadTasks",
    }
}

/// Read every persisted download task/batch JSON file.
fn collect_download_tasks(data_dir: &PathBuf) -> Value {
    let read_dir_json = |dir: PathBuf| -> Vec<Value> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match std::fs::read_to_string(&path)
                .ok()
                .and_then(|content| serde_json::from_str::<Value>(&content).ok())
            {
                Some(value) => out.push(value),
                None => eprintln!(
                    "[config] backup: skipping unreadable download state {:?}",
                    path
                ),
            }
        }
        out
    };
    json!({
        "tasks": read_dir_json(data_dir.join("downloads").join("tasks")),
        "batches": read_dir_json(data_dir.join("downloads").join("batches")),
    })
}

/// Enrich the base payload (config + accounts) into a full version-2
/// payload. Same as [`assemble_backup_payload_with_fingerprint`] but
/// discards the dynamic-content fingerprint (used by callers that always
/// write/export, like the manual export command).
pub fn assemble_backup_payload(app: &AppHandle, base_payload: Value) -> Value {
    assemble_backup_payload_with_fingerprint(app, base_payload).0
}

/// Enrich the base payload into a full version-2 payload: collects the
/// selected sections, encrypts them with the locally cached backup key, and
/// returns the payload together with a fingerprint of the dynamic sections
/// (chat history / catalog) actually included. `None` when no dynamic
/// section is part of the payload. Without a set password (or when nothing
/// is collected) the payload stays a plain config/accounts backup.
pub fn assemble_backup_payload_with_fingerprint(
    app: &AppHandle,
    mut payload: Value,
) -> (Value, Option<String>) {
    let items = items_from_payload(&payload);
    if items.is_empty() {
        payload["items"] = json!([]);
        return (payload, None);
    }

    let accounts: Vec<AccountEntry> = payload
        .get("accounts")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let (sections, collected) = collect_sections(app, &items, &accounts);
    let fingerprint = dynamic_fingerprint(&sections, &collected);
    if collected.is_empty() {
        payload["items"] = json!([]);
        return (payload, None);
    }

    match backup_crypto::load_backup_key() {
        Some((key, salt)) => match backup_crypto::encrypt_sections(&key, &salt, &sections) {
            Ok(enc) => {
                payload["items"] = serde_json::to_value(&collected).unwrap_or_else(|_| json!([]));
                payload["enc"] = serde_json::to_value(enc).unwrap_or_else(|_| json!(null));
                (payload, fingerprint)
            }
            Err(e) => {
                eprintln!("[config] backup: encryption failed ({}), exporting without optional sections", e);
                payload["items"] = json!([]);
                (payload, None)
            }
        },
        None => {
            eprintln!(
                "[config] backup: password not set on this machine, skipping optional sections"
            );
            payload["items"] = json!([]);
            (payload, None)
        }
    }
}

/// Fingerprint (SHA-256, base64) of the dynamic sections — chat history and
/// Drive Catalog — that were collected. Only these two change without a
/// config/account write, so they are the sole gate for the startup and
/// exit backup triggers. `None` when neither is included.
fn dynamic_fingerprint(sections: &Value, collected: &[BackupItem]) -> Option<String> {
    let mut dynamic = serde_json::Map::new();
    for item in collected {
        match item {
            BackupItem::ChatHistory => {
                dynamic.insert("chatHistory".to_string(), sections["chatHistory"].clone());
            }
            BackupItem::Catalog => {
                dynamic.insert("catalog".to_string(), sections["catalog"].clone());
            }
            _ => {}
        }
    }
    if dynamic.is_empty() || dynamic.values().any(Value::is_null) {
        return None;
    }
    let bytes = serde_json::to_vec(&Value::Object(dynamic)).ok()?;
    Some(B64.encode(Sha256::digest(&bytes)))
}

/// Fingerprint of the dynamic sections last uploaded to the cloud, if known.
pub fn load_dynamic_hash(data_dir: &PathBuf) -> Option<String> {
    let content = std::fs::read_to_string(data_dir.join(BACKUP_STATE_FILE_NAME)).ok()?;
    let value: Value = serde_json::from_str(&content).ok()?;
    value
        .get("dynamicHash")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Record the fingerprint of the dynamic sections just uploaded.
pub fn save_dynamic_hash(data_dir: &PathBuf, hash: &str) {
    let payload = json!({ "dynamicHash": hash });
    match serde_json::to_string_pretty(&payload) {
        Ok(json) => {
            if let Err(e) = std::fs::write(data_dir.join(BACKUP_STATE_FILE_NAME), json) {
                eprintln!("[config] backup: failed to write state file: {}", e);
            }
        }
        Err(e) => eprintln!("[config] backup: failed to serialize state file: {}", e),
    }
}

/// Restore the requested sections (intersection of what the file carries and
/// what the user ticked) into live app state. Config/accounts are NOT
/// written here — the import command saves them afterwards so a section
/// failure aborts the import before anything persistent changes.
pub fn restore_sections(
    app: &AppHandle,
    sections: &Value,
    items: &[BackupItem],
) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("cannot resolve app data dir: {}", e))?;

    for item in items {
        let section = &sections[section_key(*item)];
        if section.is_null() {
            continue;
        }
        match item {
            BackupItem::Tokens => {
                let tokens: Vec<Value> =
                    serde_json::from_value(section.clone()).map_err(|e| e.to_string())?;
                for token in tokens {
                    let home_account_id = token["homeAccountId"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let cloud_env: CloudEnvironment = serde_json::from_value(token["cloudEnv"].clone())
                        .map_err(|e| e.to_string())?;
                    let refresh_token =
                        token["refreshToken"].as_str().unwrap_or_default().to_string();
                    if home_account_id.is_empty() || refresh_token.is_empty() {
                        continue;
                    }
                    crate::auth::store_refresh_token(&cloud_env, &home_account_id, &refresh_token)
                        .map_err(|e| format!("token restore failed for {}: {}", home_account_id, e))?;
                }
            }
            BackupItem::LlmConfig => {
                let config: crate::llm::config::LlmConfig =
                    serde_json::from_value(section.clone()).map_err(|e| e.to_string())?;
                app.state::<LlmConfigManager>()
                    .save(&config)
                    .map_err(|e| e.to_string())?;
            }
            BackupItem::LlmKeys => {
                let keys: serde_json::Map<String, Value> =
                    serde_json::from_value(section.clone()).map_err(|e| e.to_string())?;
                let manager = app.state::<LlmConfigManager>();
                for (provider_id, key) in keys {
                    if let Some(key) = key.as_str() {
                        manager
                            .store_api_key(&provider_id, key)
                            .map_err(|e| format!("api key restore failed for {}: {}", provider_id, e))?;
                    }
                }
            }
            BackupItem::ChatHistory => {
                let snapshot: ChatHistorySnapshot =
                    serde_json::from_value(section.clone()).map_err(|e| e.to_string())?;
                let db_path = app
                    .try_state::<std::sync::Arc<crate::store::chat_history::ChatHistoryStore>>()
                    .map(|store| store.db_path().clone())
                    .ok_or_else(|| "chat store unavailable".to_string())?;
                import_chat_history_snapshot(&db_path, &snapshot)
                    .map_err(|e| format!("chat history restore failed: {}", e))?;
            }
            BackupItem::Catalog => {
                let snapshot: CatalogSnapshot =
                    serde_json::from_value(section.clone()).map_err(|e| e.to_string())?;
                app.try_state::<std::sync::Arc<CatalogStore>>()
                    .ok_or_else(|| "catalog store unavailable".to_string())?
                    .import_all(&snapshot)
                    .map_err(|e| format!("catalog restore failed: {}", e))?;
            }
            BackupItem::WebdavMounts => {
                let config: crate::webdav::mounts::MountConfig =
                    serde_json::from_value(section.clone()).map_err(|e| e.to_string())?;
                MountStore::new(&data_dir)
                    .save(&config)
                    .map_err(|e| format!("webdav mount restore failed: {}", e))?;
            }
            BackupItem::DownloadTasks => {
                write_download_tasks(&data_dir, section)
                    .map_err(|e| format!("download task restore failed: {}", e))?;
            }
        }
    }
    Ok(())
}

/// Write persisted download task/batch JSON files back under
/// `<data_dir>/downloads/{tasks,batches}`. The download engine picks them up
/// on the next launch (load_state runs at construction).
fn write_download_tasks(data_dir: &PathBuf, section: &Value) -> std::io::Result<()> {
    let write_dir = |dir: PathBuf, entries: &Value| -> std::io::Result<()> {
        std::fs::create_dir_all(&dir)?;
        let Some(array) = entries.as_array() else {
            return Ok(());
        };
        for entry in array {
            let Some(id) = entry["id"].as_str() else {
                continue;
            };
            let json = serde_json::to_string_pretty(entry)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            std::fs::write(dir.join(format!("{}.json", id)), json)?;
        }
        Ok(())
    };
    let downloads = data_dir.join("downloads");
    write_dir(downloads.join("tasks"), &section["tasks"])?;
    write_dir(downloads.join("batches"), &section["batches"])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::BackupItem;
    use tempfile::TempDir;

    #[test]
    fn dynamic_fingerprint_is_deterministic_and_sensitive() {
        let sections = json!({
            "chatHistory": {"conversations": [{"id": "c1", "title": "t"}]},
            "catalog": {"drives": [{"driveId": "d1"}]},
            "llmConfig": {"providers": []},
        });
        let collected = vec![BackupItem::ChatHistory, BackupItem::Catalog];
        let fp1 = dynamic_fingerprint(&sections, &collected).unwrap();
        let fp2 = dynamic_fingerprint(&sections, &collected).unwrap();
        assert_eq!(fp1, fp2, "same content must produce the same fingerprint");

        // A single changed chat row must change the fingerprint.
        let mut changed = sections.clone();
        changed["chatHistory"]["conversations"][0]["title"] = json!("edited");
        assert_ne!(
            fp1,
            dynamic_fingerprint(&changed, &collected).unwrap(),
            "chat content change must alter the fingerprint"
        );

        // Non-dynamic sections (llmConfig) must not affect the fingerprint.
        let mut llm_only = sections.clone();
        llm_only["llmConfig"]["providers"] = json!([{"id": "p1"}]);
        assert_eq!(
            fp1,
            dynamic_fingerprint(&llm_only, &collected).unwrap(),
            "llm config changes must not trigger startup/exit backups"
        );

        // Without any dynamic item collected there is no fingerprint.
        assert!(dynamic_fingerprint(&sections, &[BackupItem::LlmConfig]).is_none());
        assert!(dynamic_fingerprint(&sections, &[]).is_none());
    }

    #[test]
    fn dynamic_hash_state_file_round_trip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();

        assert_eq!(load_dynamic_hash(&path), None, "missing file = no hash");
        save_dynamic_hash(&path, "abc123");
        assert_eq!(load_dynamic_hash(&path), Some("abc123".to_string()));
        save_dynamic_hash(&path, "def456");
        assert_eq!(load_dynamic_hash(&path), Some("def456".to_string()));
    }
}
