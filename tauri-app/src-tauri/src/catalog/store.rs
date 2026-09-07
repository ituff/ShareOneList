// Drive catalog storage: SQLite schema, migrations, drive registry, and
// node upserts with an FTS5 (trigram) index kept in sync via triggers.
// The catalog is derived data — corruption is recovered by recreating the
// file; nothing here is a source of truth for the user's cloud.

use std::path::PathBuf;

use rusqlite::{params, Connection};

use crate::auth::cloud_config::CloudEnvironment;
use crate::errors::AppError;

const SCHEMA_VERSION: i64 = 1;

/// Registry entry for one indexed drive.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDrive {
    pub account_id: String,
    pub cloud_env: String,
    pub drive_id: String,
    pub kind: String,
    pub name: String,
    pub site_name: String,
    pub status: String,
    pub node_count: i64,
    pub last_used_at: i64,
}

/// A node to write into the catalog. `desc` is optional one-line context.
#[derive(Debug, Clone)]
pub struct CatalogNodeInput {
    pub path: String,
    pub item_id: String,
    pub name: String,
    pub kind: String, // 'folder' | 'file'
    pub desc: String,
}

/// SQLite-backed drive catalog.
pub struct CatalogStore {
    db_path: PathBuf,
}

pub fn env_str(env: &CloudEnvironment) -> &'static str {
    match env {
        CloudEnvironment::Global => "global",
        CloudEnvironment::China => "china",
    }
}

impl CatalogStore {
    pub fn new(base_path: PathBuf) -> Self {
        Self {
            db_path: base_path.join("catalog.db"),
        }
    }

    pub fn open(&self) -> Result<Connection, AppError> {
        let conn = Connection::open(&self.db_path).map_err(|e| AppError::Config {
            message: format!("cannot open catalog database: {}", e),
        })?;
        Self::migrate(&conn)?;
        Ok(conn)
    }

    /// Forward-only migrations keyed by `PRAGMA user_version`.
    fn migrate(conn: &Connection) -> Result<(), AppError> {
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        if version >= SCHEMA_VERSION {
            return Ok(());
        }
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS drives (
                 account_id  TEXT NOT NULL,
                 cloud_env   TEXT NOT NULL,
                 drive_id    TEXT NOT NULL,
                 kind        TEXT NOT NULL,
                 name        TEXT NOT NULL,
                 site_name   TEXT NOT NULL DEFAULT '',
                 status      TEXT NOT NULL DEFAULT 'queued',
                 node_count  INTEGER NOT NULL DEFAULT 0,
                 last_used_at INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (account_id, drive_id)
             );
             CREATE TABLE IF NOT EXISTS nodes (
                 account_id  TEXT NOT NULL,
                 drive_id    TEXT NOT NULL,
                 item_id     TEXT NOT NULL DEFAULT '',
                 path        TEXT NOT NULL,
                 name        TEXT NOT NULL,
                 kind        TEXT NOT NULL,
                 desc        TEXT NOT NULL DEFAULT '',
                 parent_path TEXT NOT NULL DEFAULT '',
                 last_visited INTEGER NOT NULL DEFAULT 0,
                 visit_count INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (account_id, drive_id, path)
             );
             CREATE INDEX IF NOT EXISTS idx_nodes_parent
                 ON nodes(account_id, drive_id, parent_path);
             CREATE VIRTUAL TABLE node_fts USING fts5(
                 name, path, desc, tokenize='trigram'
             );
             CREATE TRIGGER nodes_ai AFTER INSERT ON nodes BEGIN
                 INSERT INTO node_fts(rowid, name, path, desc)
                     VALUES (new.rowid, new.name, new.path, new.desc);
             END;
             CREATE TRIGGER nodes_ad AFTER DELETE ON nodes BEGIN
                 DELETE FROM node_fts WHERE rowid = old.rowid;
             END;
             CREATE TRIGGER nodes_au AFTER UPDATE OF name, path, desc ON nodes BEGIN
                 DELETE FROM node_fts WHERE rowid = old.rowid;
                 INSERT INTO node_fts(rowid, name, path, desc)
                     VALUES (new.rowid, new.name, new.path, new.desc);
             END;
             CREATE TABLE IF NOT EXISTS usage (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 ts INTEGER NOT NULL,
                 account_id TEXT NOT NULL,
                 source TEXT NOT NULL,
                 question TEXT NOT NULL DEFAULT '',
                 payload TEXT NOT NULL DEFAULT '[]'
             );
             PRAGMA user_version = 1;
             COMMIT;",
        )
        .map_err(|e| AppError::Config {
            message: format!("catalog migration failed: {}", e),
        })
    }

    fn now() -> i64 {
        chrono::Utc::now().timestamp()
    }

    // ── Drive registry ──────────────────────────────────────────────────────

    /// Registers a drive (idempotent on (account, drive)); returns true when
    /// newly created — the caller uses this to decide whether to seed.
    pub fn register_drive(
        &self,
        account_id: &str,
        cloud_env: &CloudEnvironment,
        drive_id: &str,
        kind: &str,
        name: &str,
        site_name: &str,
    ) -> Result<bool, AppError> {
        let mut conn = self.open()?;
        let is_new: bool = {
            let check = conn
                .query_row(
                    "SELECT 1 FROM drives WHERE account_id = ?1 AND drive_id = ?2",
                    params![account_id, drive_id],
                    |_| Ok(()),
                )
                .is_ok();
            !check
        };
        let tx = conn.transaction().map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        tx.execute(
            "INSERT INTO drives (account_id, cloud_env, drive_id, kind, name, site_name, status, last_used_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', ?7)
             ON CONFLICT(account_id, drive_id) DO UPDATE SET
                 name = excluded.name,
                 site_name = CASE WHEN excluded.site_name != '' THEN excluded.site_name ELSE drives.site_name END,
                 kind = excluded.kind,
                 last_used_at = excluded.last_used_at",
            params![
                account_id,
                env_str(cloud_env),
                drive_id,
                kind,
                name,
                site_name,
                Self::now()
            ],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        tx.commit().map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        Ok(is_new)
    }

    /// Registers a drive only when it does not exist yet (used by writebacks,
    /// which may fire before an explicit registration).
    pub fn register_drive_if_missing(
        &self,
        account_id: &str,
        cloud_env: &CloudEnvironment,
        drive_id: &str,
    ) -> Result<(), AppError> {
        if self.drive_status(account_id, drive_id)?.is_none() {
            self.register_drive(
                account_id,
                cloud_env,
                drive_id,
                "unknown",
                drive_id,
                "",
            )?;
        } else {
            let conn = self.open()?;
            conn.execute(
                "UPDATE drives SET last_used_at = ?3 WHERE account_id = ?1 AND drive_id = ?2",
                params![account_id, drive_id, Self::now()],
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        }
        Ok(())
    }

    /// Deletes a drive and all its nodes. Unknown drives are fine.
    pub fn unregister_drive(&self, account_id: &str, drive_id: &str) -> Result<(), AppError> {
        let conn = self.open()?;
        conn.execute(
            "DELETE FROM nodes WHERE account_id = ?1 AND drive_id = ?2",
            params![account_id, drive_id],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        conn.execute(
            "DELETE FROM drives WHERE account_id = ?1 AND drive_id = ?2",
            params![account_id, drive_id],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        Ok(())
    }

    pub fn list_drives(&self) -> Result<Vec<CatalogDrive>, AppError> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT account_id, cloud_env, drive_id, kind, name, site_name, status, node_count, last_used_at
                 FROM drives ORDER BY last_used_at DESC",
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CatalogDrive {
                    account_id: row.get(0)?,
                    cloud_env: row.get(1)?,
                    drive_id: row.get(2)?,
                    kind: row.get(3)?,
                    name: row.get(4)?,
                    site_name: row.get(5)?,
                    status: row.get(6)?,
                    node_count: row.get(7)?,
                    last_used_at: row.get(8)?,
                })
            })
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })
    }

    pub fn set_status(
        &self,
        account_id: &str,
        drive_id: &str,
        status: &str,
    ) -> Result<(), AppError> {
        let conn = self.open()?;
        conn.execute(
            "UPDATE drives SET status = ?3 WHERE account_id = ?1 AND drive_id = ?2",
            params![account_id, drive_id, status],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        Ok(())
    }

    pub fn drive_status(
        &self,
        account_id: &str,
        drive_id: &str,
    ) -> Result<Option<String>, AppError> {
        let conn = self.open()?;
        let status: Option<String> = conn
            .query_row(
                "SELECT status FROM drives WHERE account_id = ?1 AND drive_id = ?2",
                params![account_id, drive_id],
                |row| row.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(AppError::Config {
                    message: other.to_string(),
                }),
            })?;
        Ok(status)
    }

    pub fn refresh_node_count(&self, account_id: &str, drive_id: &str) -> Result<i64, AppError> {
        let conn = self.open()?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE account_id = ?1 AND drive_id = ?2",
                params![account_id, drive_id],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        conn.execute(
            "UPDATE drives SET node_count = ?3 WHERE account_id = ?1 AND drive_id = ?2",
            params![account_id, drive_id, count],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        Ok(count)
    }

    // ── Nodes ───────────────────────────────────────────────────────────────

    /// Upserts one node, adding `visit_delta` to its access counter and
    /// filling an empty `desc` from the input. Existing non-empty item_id,
    /// desc and accumulated visit_count survive the merge (Property 2/4).
    pub fn upsert_node(
        conn: &Connection,
        account_id: &str,
        drive_id: &str,
        node: &CatalogNodeInput,
        visit_delta: i64,
    ) -> Result<(), AppError> {
        let parent = match node.path.rfind('/') {
            Some(pos) => node.path[..pos].to_string(),
            None => String::new(),
        };
        conn.execute(
            "INSERT INTO nodes (account_id, drive_id, item_id, path, name, kind, desc, parent_path, last_visited, visit_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(account_id, drive_id, path) DO UPDATE SET
                 item_id = CASE WHEN excluded.item_id = '' THEN nodes.item_id ELSE excluded.item_id END,
                 name = excluded.name,
                 kind = excluded.kind,
                 parent_path = excluded.parent_path,
                 last_visited = CASE WHEN ?10 > 0 THEN ?9 ELSE nodes.last_visited END,
                 visit_count = nodes.visit_count + ?10,
                 desc = CASE WHEN nodes.desc = '' AND excluded.desc != '' THEN excluded.desc ELSE nodes.desc END",
            params![
                account_id,
                drive_id,
                node.item_id,
                node.path,
                node.name,
                node.kind,
                node.desc,
                parent,
                Self::now(),
                visit_delta
            ],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        Ok(())
    }

    /// Upserts a batch of nodes inside one transaction. On success refreshes
    /// the drive's node_count; on failure nothing is written.
    pub fn upsert_nodes(
        &self,
        account_id: &str,
        drive_id: &str,
        nodes: &[CatalogNodeInput],
        visit_delta: i64,
    ) -> Result<(), AppError> {
        let writes: Vec<(CatalogNodeInput, i64)> =
            nodes.iter().map(|n| (n.clone(), visit_delta)).collect();
        self.upsert_nodes_mixed(account_id, drive_id, &writes)
    }

    /// Batch upsert where each node carries its own visit delta (0 = sync
    /// metadata only, >0 = count visits). Single transaction.
    pub fn upsert_nodes_mixed(
        &self,
        account_id: &str,
        drive_id: &str,
        writes: &[(CatalogNodeInput, i64)],
    ) -> Result<(), AppError> {
        let mut conn = self.open()?;
        let tx = conn.transaction().map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        for (node, delta) in writes {
            Self::upsert_node(&tx, account_id, drive_id, node, *delta)?;
        }
        tx.commit().map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        self.refresh_node_count(account_id, drive_id)?;
        Ok(())
    }

    pub fn delete_drive_nodes(&self, account_id: &str, drive_id: &str) -> Result<(), AppError> {
        let conn = self.open()?;
        conn.execute(
            "DELETE FROM nodes WHERE account_id = ?1 AND drive_id = ?2",
            params![account_id, drive_id],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        self.refresh_node_count(account_id, drive_id)?;
        Ok(())
    }

    // ── Usage ledger ────────────────────────────────────────────────────────

    pub fn record_usage(
        &self,
        account_id: &str,
        source: &str,
        question: &str,
        payload: &serde_json::Value,
    ) -> Result<(), AppError> {
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO usage (ts, account_id, source, question, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                Self::now(),
                account_id,
                source,
                question,
                serde_json::to_string(payload).unwrap_or_default()
            ],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_store(dir: &TempDir) -> CatalogStore {
        CatalogStore::new(dir.path().to_path_buf())
    }

    fn node(path: &str, name: &str, kind: &str) -> CatalogNodeInput {
        CatalogNodeInput {
            path: path.into(),
            item_id: format!("id-{}", path),
            name: name.into(),
            kind: kind.into(),
            desc: String::new(),
        }
    }

    // Feature: drive-catalog, Property 1: registry unique per (account, drive)
    #[test]
    fn register_drive_is_idempotent_per_account_and_drive() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);

        assert!(store
            .register_drive("acc1", &CloudEnvironment::Global, "drv1", "onedrive", "OneDrive", "")
            .unwrap());
        // Same drive again: refresh only.
        assert!(!store
            .register_drive("acc1", &CloudEnvironment::Global, "drv1", "onedrive", "OneDrive 2", "")
            .unwrap());
        // Another account, same drive id: separate row (isolation).
        assert!(store
            .register_drive("acc2", &CloudEnvironment::China, "drv1", "onedrive", "CN", "")
            .unwrap());

        assert_eq!(store.list_drives().unwrap().len(), 2);
        let drv = &store.list_drives().unwrap()[0];
        assert_eq!(drv.name, "OneDrive 2", "metadata must refresh");
    }

    // Feature: drive-catalog, Property 2: node upsert idempotent, stats survive
    #[test]
    fn upsert_is_idempotent_and_preserves_stats() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store
            .register_drive("acc1", &CloudEnvironment::Global, "drv1", "onedrive", "OD", "")
            .unwrap();

        let mut n = node("R&D/report.pdf", "report.pdf", "file");
        n.desc = "2025 report".into();
        store
            .upsert_nodes("acc1", "drv1", &[n.clone()], 1)
            .unwrap();
        store
            .upsert_nodes("acc1", "drv1", &[n.clone()], 1)
            .unwrap();

        // Rename on the wire (drive-side change) must not duplicate the row.
        let renamed = CatalogNodeInput {
            name: "report-final.pdf".into(),
            item_id: "id-R&D/report.pdf".into(),
            ..node("R&D/report.pdf", "report.pdf", "file")
        };
        store.upsert_nodes("acc1", "drv1", &[renamed], 0).unwrap();

        let conn = store.open().unwrap();
        let (count, visits, desc, item_id, name): (i64, i64, String, String, String) = conn
            .query_row(
                "SELECT COUNT(*), visit_count, desc, item_id, name FROM nodes
                 WHERE account_id='acc1' AND drive_id='drv1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(visits, 2, "visits only increment when delta > 0");
        assert_eq!(desc, "2025 report");
        assert_eq!(item_id, "id-R&D/report.pdf");
        assert_eq!(name, "report-final.pdf", "wire rename must refresh name");
    }

    // Feature: drive-catalog, Property 3: FTS stays in sync across DML
    #[test]
    fn fts_tracks_insert_update_delete() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store
            .register_drive("acc1", &CloudEnvironment::Global, "drv1", "onedrive", "OD", "")
            .unwrap();

        store
            .upsert_nodes("acc1", "drv1", &[node("R&D/notes.md", "notes.md", "file")], 0)
            .unwrap();
        let conn = store.open().unwrap();
        let fts_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM node_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fts_rows, 1);

        // Update keeps the mirror at one row with the new content searchable.
        let renamed = CatalogNodeInput {
            name: "meeting-notes.md".into(),
            item_id: "id-R&D/notes.md".into(),
            ..node("R&D/notes.md", "notes.md", "file")
        };
        drop(conn);
        store.upsert_nodes("acc1", "drv1", &[renamed], 0).unwrap();
        let conn = store.open().unwrap();
        let fts_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM node_fts", [], |r| r.get(0))
            .unwrap();
        let hit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM node_fts WHERE node_fts MATCH '\"meeting-notes\"'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_rows, 1);
        assert_eq!(hit, 1);

        // Delete removes the mirror row.
        drop(conn);
        store.delete_drive_nodes("acc1", "drv1").unwrap();
        let conn = store.open().unwrap();
        let fts_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM node_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fts_rows, 0);
    }

    #[test]
    fn migrations_are_idempotent() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.open().unwrap();
        store.open().unwrap();
        assert!(store.list_drives().unwrap().is_empty());
    }

    #[test]
    fn unregister_removes_nodes() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store
            .register_drive("acc1", &CloudEnvironment::Global, "drv1", "onedrive", "OD", "")
            .unwrap();
        store
            .upsert_nodes("acc1", "drv1", &[node("a.txt", "a.txt", "file")], 0)
            .unwrap();
        store.unregister_drive("acc1", "drv1").unwrap();
        assert!(store.list_drives().unwrap().is_empty());
        let conn = store.open().unwrap();
        let nodes: i64 = conn
            .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(nodes, 0);
    }
}
