// User memory persistence: durable facts and preferences distilled from
// conversations, stored in chat_history.db (memories table, migration v2).

use std::path::PathBuf;

use rusqlite::{params, OptionalExtension};

use crate::errors::AppError;
use crate::store::chat_history::ChatHistoryStore;

/// One persisted memory entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub source_conversation_id: String,
    pub enabled: bool,
    pub pinned: bool,
    pub use_count: i64,
    pub last_used_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// SQLite-backed user memory store sharing chat_history.db.
pub struct MemoryStore {
    db_path: PathBuf,
}

impl MemoryStore {
    pub fn new(base_path: PathBuf) -> Self {
        Self {
            db_path: base_path.join("chat_history.db"),
        }
    }

    fn open(&self) -> Result<rusqlite::Connection, AppError> {
        // Runs through ChatHistoryStore so the shared file is migrated to the
        // current version (memories table lives in v2) no matter which store
        // opens first.
        let conn = rusqlite::Connection::open(&self.db_path).map_err(|e| {
            AppError::Config {
                message: format!("cannot open memory database: {}", e),
            }
        })?;
        ChatHistoryStore::migrate(&conn)?;
        Ok(conn)
    }

    /// Path of the shared chat-history database.
    pub(crate) fn chat_history_path(&self) -> &PathBuf {
        &self.db_path
    }

    fn now() -> i64 {
        chrono::Utc::now().timestamp()
    }

    fn new_id() -> String {
        format!("mem_{}", uuid::Uuid::new_v4().simple())
    }

    fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<MemoryEntry> {
        Ok(MemoryEntry {
            id: row.get(0)?,
            content: row.get(1)?,
            source_conversation_id: row.get(2)?,
            enabled: row.get::<_, i64>(3)? != 0,
            pinned: row.get::<_, i64>(4)? != 0,
            use_count: row.get(5)?,
            last_used_at: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }

    /// Lists all memories: pinned first, then most recently used.
    pub fn list(&self) -> Result<Vec<MemoryEntry>, AppError> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, content, source_conversation_id, enabled, pinned,
                        use_count, last_used_at, created_at, updated_at
                 FROM memories
                 WHERE source_conversation_id != '__counter'
                 ORDER BY pinned DESC, last_used_at DESC, created_at DESC",
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        let rows = stmt
            .query_map([], Self::row_to_entry)
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })
    }

    /// Creates or updates a memory. Empty `id` creates a new entry.
    pub fn save(
        &self,
        id: &str,
        content: &str,
        source_conversation_id: &str,
    ) -> Result<String, AppError> {
        if content.trim().is_empty() {
            return Err(AppError::Validation {
                message: "memory content must not be empty".into(),
                field: "content".into(),
            });
        }
        let conn = self.open()?;
        if id.is_empty() {
            let new_id = Self::new_id();
            let now = Self::now();
            conn.execute(
                "INSERT INTO memories (id, content, source_conversation_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![new_id, content.trim(), source_conversation_id, now],
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
            Ok(new_id)
        } else {
            conn.execute(
                "UPDATE memories SET content = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, content.trim(), Self::now()],
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
            Ok(id.to_string())
        }
    }

    /// Hard-deletes a memory (Property 3); unknown ids are fine.
    pub fn delete(&self, id: &str) -> Result<(), AppError> {
        let conn = self.open()?;
        conn.execute("DELETE FROM memories WHERE id = ?1", params![id])
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        Ok(())
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), AppError> {
        let conn = self.open()?;
        conn.execute(
            "UPDATE memories SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, enabled as i64, Self::now()],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        Ok(())
    }

    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<(), AppError> {
        let conn = self.open()?;
        conn.execute(
            "UPDATE memories SET pinned = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, pinned as i64, Self::now()],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        Ok(())
    }

    /// Deletes memories whose content contains the keyword; returns the
    /// number of rows removed (`#forget` support).
    pub fn search_and_delete(&self, keyword: &str) -> Result<usize, AppError> {
        let conn = self.open()?;
        let pattern = format!("%{}%", keyword.replace('%', "\\%"));
        let count = conn
            .execute(
                "DELETE FROM memories WHERE content LIKE ?1 ESCAPE '\\'",
                params![pattern],
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        Ok(count)
    }

    /// Enabled memories for prompt injection, ordered pinned-first then most
    /// recently used, capped at `limit`.
    pub fn enabled_for_injection(&self, limit: usize) -> Result<Vec<MemoryEntry>, AppError> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, content, source_conversation_id, enabled, pinned,
                        use_count, last_used_at, created_at, updated_at
                 FROM memories
                 WHERE enabled = 1 AND source_conversation_id != '__counter'
                 ORDER BY pinned DESC, last_used_at DESC, created_at DESC
                 LIMIT ?1",
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        let rows = stmt
            .query_map(params![limit as i64], Self::row_to_entry)
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })
    }

    /// Refreshes usage stats after a memory was injected.
    pub fn touch_used(&self, ids: &[String]) {
        if ids.is_empty() {
            return;
        }
        if let Ok(conn) = self.open() {
            let now = Self::now();
            for id in ids {
                let _ = conn.execute(
                    "UPDATE memories SET use_count = use_count + 1, last_used_at = ?2 WHERE id = ?1",
                    params![id, now],
                );
            }
        }
    }

    /// Replaces all auto-generated memories (non-manual source, not pinned)
    /// with a fresh list, inheriting usage stats from entries whose content
    /// is unchanged (Property 5/6). Manual and pinned entries survive.
    pub fn replace_auto_memories(&self, contents: &[String], source_conversation_id: &str) -> Result<usize, AppError> {
        let mut conn = self.open()?;
        let now = Self::now();
        let tx = conn.transaction().map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;

        // Snapshot existing auto entries to inherit stats by content match.
        let mut stmt = tx
            .prepare(
                "SELECT id, content, use_count, last_used_at FROM memories
                 WHERE source_conversation_id != 'manual' AND pinned = 0",
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        let previous: Vec<(String, String, i64, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        drop(stmt);

        tx.execute(
            "DELETE FROM memories WHERE source_conversation_id != 'manual' AND pinned = 0",
            [],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;

        let mut inserted = 0;
        for content in contents {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Inherit stats from a same-content predecessor.
            let inherited = previous
                .iter()
                .find(|(_, old_content, _, _)| old_content == trimmed);
            let (id, use_count, last_used_at) = match inherited {
                Some((old_id, _, old_count, old_used)) => {
                    (old_id.clone(), *old_count, *old_used)
                }
                None => (Self::new_id(), 0, 0),
            };
            tx.execute(
                "INSERT INTO memories (id, content, source_conversation_id, enabled, pinned, use_count, last_used_at, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 1, 0, ?4, ?5, ?6, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                     content = excluded.content,
                     updated_at = excluded.updated_at",
                params![id, trimmed, source_conversation_id, use_count, last_used_at, now],
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
            inserted += 1;
        }
        tx.commit().map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        Ok(inserted)
    }

    /// Counts user messages appended since the last extraction marker.
    /// Markers are stored as memories with a reserved id prefix — simple and
    /// survives restarts without a separate table.
    pub fn user_messages_since_extract(&self, conversation_id: &str) -> Result<usize, AppError> {
        let conn = self.open()?;
        let key = format!("__counter_{}", conversation_id);
        let value: Option<String> = conn
            .query_row(
                "SELECT content FROM memories WHERE id = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        Ok(value.and_then(|v| v.parse().ok()).unwrap_or(0))
    }

    pub fn set_user_messages_since_extract(
        &self,
        conversation_id: &str,
        count: usize,
    ) -> Result<(), AppError> {
        let conn = self.open()?;
        let key = format!("__counter_{}", conversation_id);
        conn.execute(
            "INSERT INTO memories (id, content, source_conversation_id, created_at, updated_at)
             VALUES (?1, ?2, '__counter', ?3, ?3)
             ON CONFLICT(id) DO UPDATE SET content = excluded.content",
            params![key, count.to_string(), Self::now()],
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

    fn make_store(dir: &TempDir) -> MemoryStore {
        MemoryStore::new(dir.path().to_path_buf())
    }

    #[test]
    fn save_list_delete_round_trip() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        let id = store.save("", "用户常用 DeepSeek", "conv1").unwrap();

        let all = store.list().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].content, "用户常用 DeepSeek");
        assert!(all[0].enabled);
        assert!(!all[0].pinned);

        // Edit keeps the same id.
        let id2 = store.save(&id, "用户偏好 DeepSeek 模型", "conv1").unwrap();
        assert_eq!(id, id2);
        assert_eq!(store.list().unwrap()[0].content, "用户偏好 DeepSeek 模型");

        // Property 3: hard delete removes it for good.
        store.delete(&id).unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn save_rejects_empty_content() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        assert!(store.save("", "   ", "").is_err());
    }

    #[test]
    fn enabled_filter_and_pinned_ordering() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        let a = store.save("", "记忆 A", "").unwrap();
        let b = store.save("", "记忆 B", "").unwrap();
        let c = store.save("", "记忆 C", "").unwrap();
        store.set_enabled(&b, false).unwrap();
        store.set_pinned(&a, true).unwrap();

        let injectable: Vec<String> = store
            .enabled_for_injection(20)
            .unwrap()
            .into_iter()
            .map(|m| m.content)
            .collect();
        assert_eq!(injectable, vec!["记忆 A", "记忆 C"], "disabled excluded, pinned first");
        let _ = c;
    }

    // Feature: ai-memory, Property 5: manual and pinned entries survive merges
    #[test]
    fn replace_auto_preserves_manual_and_pinned() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        let manual = store.save("", "手动条目", "manual").unwrap();
        let auto_old = store.save("", "旧自动条目", "conv1").unwrap();
        store.set_pinned(&auto_old, true).unwrap();

        store
            .replace_auto_memories(&["新自动条目 1".into(), "新自动条目 2".into()], "conv2")
            .unwrap();

        let contents: Vec<String> = store.list().unwrap().into_iter().map(|m| m.content).collect();
        assert!(contents.contains(&"手动条目".to_string()));
        assert!(contents.contains(&"旧自动条目".to_string()), "pinned auto survives");
        assert!(contents.contains(&"新自动条目 1".to_string()));
        assert!(contents.contains(&"新自动条目 2".to_string()));
        assert_eq!(contents.len(), 4);
        let _ = manual;
    }

    // Feature: ai-memory, Property 6: stats inherit across same-content merges
    #[test]
    fn replace_auto_inherits_stats_for_unchanged_content() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        let id = store.save("", "稳定条目", "conv1").unwrap();
        store.touch_used(&[id.clone()]);
        store.touch_used(&[id.clone()]);

        store
            .replace_auto_memories(&["稳定条目".into(), "新增".into()], "conv2")
            .unwrap();

        let entry = store
            .list()
            .unwrap()
            .into_iter()
            .find(|m| m.content == "稳定条目")
            .unwrap();
        assert_eq!(entry.use_count, 2, "use_count must survive the merge");
        assert!(entry.last_used_at > 0);
        // The inherited entry keeps its id (id-based ON CONFLICT upsert).
        assert_eq!(entry.id, id);
    }

    #[test]
    fn search_and_delete_counts_and_removes() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.save("", "用户常用 DeepSeek 模型", "").unwrap();
        store.save("", "用户在福斯工作", "").unwrap();
        store.save("", "喜欢简洁回答", "").unwrap();

        assert_eq!(store.search_and_delete("DeepSeek").unwrap(), 1);
        assert_eq!(store.search_and_delete("不存在的关键词").unwrap(), 0);
        assert_eq!(store.list().unwrap().len(), 2);
    }

    #[test]
    fn extraction_counter_persists() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        assert_eq!(store.user_messages_since_extract("conv1").unwrap(), 0);
        store.set_user_messages_since_extract("conv1", 6).unwrap();
        assert_eq!(store.user_messages_since_extract("conv1").unwrap(), 6);
        // Counters live in the same table; reset to 0 keeps the row but
        // must not leak into enabled_for_injection.
        store.set_user_messages_since_extract("conv1", 0).unwrap();
        assert!(store.enabled_for_injection(20).unwrap().is_empty());
    }
}
