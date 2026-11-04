// Chat history persistence: SQLite-backed conversation and message storage.
// The database is fully derived data — corruption is recovered by recreating
// the file, consistent with the layered error strategy.

use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension};

use crate::auth::cloud_config::CloudEnvironment;
use crate::errors::AppError;
use crate::models::DriveItem;

/// A cloud file attached to a chat message (citation chips). Mirrors the
/// frontend `ContextFile` shape exactly so it round-trips without mapping.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredContextFile {
    pub item: DriveItem,
    pub drive_id: String,
    pub cloud_env: CloudEnvironment,
    pub home_account_id: String,
    pub account_name: String,
    pub path: String,
    #[serde(default)]
    pub excerpt: Option<String>,
}

/// One persisted chat message.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredChatMessage {
    /// "user" | "assistant"
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub context_files: Vec<StoredContextFile>,
    #[serde(default)]
    pub created_at: i64,
}

/// Conversation list entry.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    pub updated_at: i64,
}

/// A conversation with its full message list.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDetail {
    pub id: String,
    pub messages: Vec<StoredChatMessage>,
}

/// SQLite-backed chat history store.
pub struct ChatHistoryStore {
    db_path: PathBuf,
}

impl ChatHistoryStore {
    pub fn new(base_path: PathBuf) -> Self {
        Self {
            db_path: base_path.join("chat_history.db"),
        }
    }

    /// Path of the underlying database file (shared with the memory store).
    pub(crate) fn db_path(&self) -> &PathBuf {
        &self.db_path
    }

    fn open(&self) -> Result<Connection, AppError> {
        let conn = Connection::open(&self.db_path).map_err(|e| AppError::Config {
            message: format!("cannot open chat history database: {}", e),
        })?;
        Self::migrate(&conn)?;
        Ok(conn)
    }

    /// Schema migrations keyed by `PRAGMA user_version`; forward-only, each
    /// step idempotent so an interrupted upgrade can resume.
    pub(crate) fn migrate(conn: &Connection) -> Result<(), AppError> {
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        if version < 1 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS conversations (
                     id TEXT PRIMARY KEY,
                     title TEXT NOT NULL DEFAULT '',
                     created_at INTEGER NOT NULL,
                     updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS messages (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                     role TEXT NOT NULL,
                     content TEXT NOT NULL DEFAULT '',
                     reasoning TEXT,
                     context_files TEXT NOT NULL DEFAULT '[]',
                     created_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS idx_messages_conversation
                     ON messages(conversation_id, id);
                 PRAGMA user_version = 1;
                 COMMIT;",
            )
            .map_err(|e| AppError::Config {
                message: format!("chat history migration v1 failed: {}", e),
            })?;
        }
        if version < 2 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS memories (
                     id TEXT PRIMARY KEY,
                     content TEXT NOT NULL,
                     source_conversation_id TEXT NOT NULL DEFAULT '',
                     enabled INTEGER NOT NULL DEFAULT 1,
                     pinned INTEGER NOT NULL DEFAULT 0,
                     use_count INTEGER NOT NULL DEFAULT 0,
                     last_used_at INTEGER NOT NULL DEFAULT 0,
                     created_at INTEGER NOT NULL,
                     updated_at INTEGER NOT NULL
                 );
                 PRAGMA user_version = 2;
                 COMMIT;",
            )
            .map_err(|e| AppError::Config {
                message: format!("chat history migration v2 failed: {}", e),
            })?;
        }
        Ok(())
    }

    fn now() -> i64 {
        chrono::Utc::now().timestamp()
    }

    fn new_id() -> String {
        format!("conv_{}", uuid::Uuid::new_v4().simple())
    }

    fn insert_conversation(conn: &Connection, title: &str) -> Result<String, AppError> {
        let id = Self::new_id();
        let now = Self::now();
        conn.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            params![id, title, now],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        Ok(id)
    }

    /// Returns the most recently updated conversation, creating one when the
    /// store is empty. The app always has a conversation to write into.
    pub fn open_latest_or_create(&self) -> Result<String, AppError> {
        let conn = self.open()?;
        let id: Option<String> = conn
            .query_row(
                "SELECT id FROM conversations ORDER BY updated_at DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        match id {
            Some(id) => Ok(id),
            None => Self::insert_conversation(&conn, ""),
        }
    }

    pub fn list_conversations(&self) -> Result<Vec<ConversationMeta>, AppError> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare("SELECT id, title, updated_at FROM conversations ORDER BY updated_at DESC")
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ConversationMeta {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    updated_at: row.get(2)?,
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

    pub fn create_conversation(&self, title: &str) -> Result<String, AppError> {
        let conn = self.open()?;
        Self::insert_conversation(&conn, title)
    }

    pub fn get_messages(&self, conversation_id: &str) -> Result<Vec<StoredChatMessage>, AppError> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT role, content, reasoning, context_files, created_at
                 FROM messages WHERE conversation_id = ?1 ORDER BY id",
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        let rows = stmt
            .query_map(params![conversation_id], |row| {
                let context_json: String = row.get(3)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    context_json,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;

        let mut messages = Vec::new();
        for row in rows {
            let (role, content, reasoning, context_json, created_at) =
                row.map_err(|e| AppError::Config {
                    message: e.to_string(),
                })?;
            let context_files: Vec<StoredContextFile> = serde_json::from_str(&context_json)
                .unwrap_or_default();
            messages.push(StoredChatMessage {
                role,
                content,
                reasoning,
                context_files,
                created_at,
            });
        }
        Ok(messages)
    }

    /// Loads a conversation and its messages; returns `None` for unknown ids.
    pub fn open_conversation(&self, conversation_id: &str) -> Result<Option<ConversationDetail>, AppError> {
        let conn = self.open()?;
        let exists: Option<String> = conn
            .query_row(
                "SELECT id FROM conversations WHERE id = ?1",
                params![conversation_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        if exists.is_none() {
            return Ok(None);
        }
        Ok(Some(ConversationDetail {
            id: conversation_id.to_string(),
            messages: self.get_messages(conversation_id)?,
        }))
    }

    pub fn append_message(
        &self,
        conversation_id: &str,
        message: &StoredChatMessage,
    ) -> Result<(), AppError> {
        let conn = self.open()?;
        let created_at = if message.created_at > 0 {
            message.created_at
        } else {
            Self::now()
        };
        let context_json = serde_json::to_string(&message.context_files)
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        conn.execute(
            "INSERT INTO messages (conversation_id, role, content, reasoning, context_files, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                conversation_id,
                message.role,
                message.content,
                message.reasoning,
                context_json,
                created_at
            ],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;

        // Auto-title from the first user message of an untitled conversation.
        let title: String = conn
            .query_row(
                "SELECT title FROM conversations WHERE id = ?1",
                params![conversation_id],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        if title.is_empty() && message.role == "user" {
            let auto: String = message.content.chars().take(30).collect();
            conn.execute(
                "UPDATE conversations SET title = ?2, updated_at = ?3 WHERE id = ?1",
                params![conversation_id, auto, Self::now()],
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        } else {
            conn.execute(
                "UPDATE conversations SET updated_at = ?2 WHERE id = ?1",
                params![conversation_id, Self::now()],
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        }
        Ok(())
    }

    /// Deletes a conversation and its messages. Unknown ids are fine.
    pub fn delete_conversation(&self, conversation_id: &str) -> Result<(), AppError> {
        let conn = self.open()?;
        conn.execute(
            "DELETE FROM messages WHERE conversation_id = ?1",
            params![conversation_id],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        conn.execute(
            "DELETE FROM conversations WHERE id = ?1",
            params![conversation_id],
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        Ok(())
    }
}

/// Reads the last `n` messages of a conversation as chat messages for the
/// memory-extraction window (static so the memory extractor can use it
/// without owning a ChatHistoryStore).
pub(crate) fn recent_messages_for_extraction(
    db_path: &PathBuf,
    conversation_id: &str,
    n: usize,
) -> Vec<crate::llm::client::LlmChatMessage> {
    let Ok(conn) = Connection::open(db_path) else {
        return Vec::new();
    };
    if ChatHistoryStore::migrate(&conn).is_err() {
        return Vec::new();
    }
    let mut stmt = match conn.prepare(
        "SELECT role, content FROM messages
         WHERE conversation_id = ?1 AND content != ''
         ORDER BY id DESC LIMIT ?2",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(params![conversation_id, n as i64], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };
    let mut messages: Vec<crate::llm::client::LlmChatMessage> = rows
        .filter_map(|r| r.ok())
        .map(|(role, content)| crate::llm::client::LlmChatMessage { role, content })
        .collect();
    messages.reverse();
    messages
}

/// One raw conversation row in a backup snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotConversation {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One raw message row in a backup snapshot (ids preserved).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotMessage {
    pub id: i64,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub reasoning: Option<String>,
    /// Raw JSON string of the context file list.
    #[serde(default)]
    pub context_files: String,
    pub created_at: i64,
}

/// One raw memory row in a backup snapshot, including the internal
/// `__counter_*` extraction-marker rows.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotMemory {
    pub id: String,
    pub content: String,
    #[serde(default)]
    pub source_conversation_id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub use_count: i64,
    #[serde(default)]
    pub last_used_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

fn default_true() -> bool {
    true
}

/// Full snapshot of chat_history.db: conversations, messages and user
/// memories. Used by the backup feature; restore is a transactional
/// clear-and-insert that preserves ids and timestamps.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChatHistorySnapshot {
    pub conversations: Vec<SnapshotConversation>,
    pub messages: Vec<SnapshotMessage>,
    pub memories: Vec<SnapshotMemory>,
}

/// Export every row of chat_history.db (conversations, messages, memories)
/// as a portable snapshot.
pub fn export_chat_history_snapshot(db_path: &PathBuf) -> Result<ChatHistorySnapshot, AppError> {
    let conn = Connection::open(db_path).map_err(|e| AppError::Config {
        message: format!("cannot open chat history database: {}", e),
    })?;
    ChatHistoryStore::migrate(&conn)?;

    let mut conversations = Vec::new();
    let mut stmt = conn
        .prepare("SELECT id, title, created_at, updated_at FROM conversations ORDER BY created_at")
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
    let rows = stmt
        .query_map([], |row| {
            Ok(SnapshotConversation {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
    for row in rows {
        conversations.push(row.map_err(|e| AppError::Config {
            message: e.to_string(),
        })?);
    }

    let mut messages = Vec::new();
    let mut stmt = conn
        .prepare(
            "SELECT id, conversation_id, role, content, reasoning, context_files, created_at
             FROM messages ORDER BY id",
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
    let rows = stmt
        .query_map([], |row| {
            Ok(SnapshotMessage {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                reasoning: row.get(4)?,
                context_files: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
    for row in rows {
        messages.push(row.map_err(|e| AppError::Config {
            message: e.to_string(),
        })?);
    }

    let mut memories = Vec::new();
    let mut stmt = conn
        .prepare(
            "SELECT id, content, source_conversation_id, enabled, pinned, use_count,
                    last_used_at, created_at, updated_at
             FROM memories ORDER BY created_at, id",
        )
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
    let rows = stmt
        .query_map([], |row| {
            Ok(SnapshotMemory {
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
        })
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
    for row in rows {
        memories.push(row.map_err(|e| AppError::Config {
            message: e.to_string(),
        })?);
    }

    Ok(ChatHistorySnapshot {
        conversations,
        messages,
        memories,
    })
}

/// Replace the contents of chat_history.db with `snapshot` in a single
/// transaction (clear + insert, ids and timestamps preserved). Concurrent
/// writers get SQLITE_BUSY after the busy timeout rather than corruption.
pub fn import_chat_history_snapshot(
    db_path: &PathBuf,
    snapshot: &ChatHistorySnapshot,
) -> Result<(), AppError> {
    let mut conn = Connection::open(db_path).map_err(|e| AppError::Config {
        message: format!("cannot open chat history database: {}", e),
    })?;
    ChatHistoryStore::migrate(&conn)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;

    let tx = conn.transaction().map_err(|e| AppError::Config {
        message: e.to_string(),
    })?;
    tx.execute("DELETE FROM messages", [])
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
    tx.execute("DELETE FROM conversations", [])
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
    tx.execute("DELETE FROM memories", [])
        .map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
    // Reset the AUTOINCREMENT counter so restored message ids stay stable
        // and new messages continue above the highest restored id.
    let _ = tx.execute("DELETE FROM sqlite_sequence WHERE name = 'messages'", []);

    {
        let mut stmt = tx
            .prepare("INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)")
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        for c in &snapshot.conversations {
            stmt.execute(params![c.id, c.title, c.created_at, c.updated_at])
                .map_err(|e| AppError::Config {
                    message: e.to_string(),
                })?;
        }
    }
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO messages (id, conversation_id, role, content, reasoning, context_files, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        for m in &snapshot.messages {
            stmt.execute(params![
                m.id,
                m.conversation_id,
                m.role,
                m.content,
                m.reasoning,
                m.context_files,
                m.created_at
            ])
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        }
    }
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO memories (id, content, source_conversation_id, enabled, pinned, use_count, last_used_at, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        for m in &snapshot.memories {
            stmt.execute(params![
                m.id,
                m.content,
                m.source_conversation_id,
                m.enabled as i64,
                m.pinned as i64,
                m.use_count,
                m.last_used_at,
                m.created_at,
                m.updated_at
            ])
            .map_err(|e| AppError::Config {
                message: e.to_string(),
            })?;
        }
    }

    tx.commit().map_err(|e| AppError::Config {
        message: e.to_string(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_store(dir: &TempDir) -> ChatHistoryStore {
        ChatHistoryStore::new(dir.path().to_path_buf())
    }

    fn user_msg(content: &str) -> StoredChatMessage {
        StoredChatMessage {
            role: "user".into(),
            content: content.into(),
            reasoning: None,
            context_files: vec![],
            created_at: 0,
        }
    }

    // Feature: ai-assistant, Property 8: chat history round-trip
    #[test]
    fn messages_round_trip_with_context_files() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        let conv_id = store.open_latest_or_create().unwrap();

        let context = StoredContextFile {
            item: DriveItem {
                id: "item-1".into(),
                name: "notes.md".into(),
                size: Some(120),
                last_modified: "2026-09-01T00:00:00Z".into(),
                is_folder: false,
                mime_type: None,
                web_url: Some("https://x".into()),
                parent_reference: None,
                download_url: None,
                created_date_time: None,
            },
            drive_id: "drive-1".into(),
            cloud_env: CloudEnvironment::Global,
            home_account_id: "acc-1".into(),
            account_name: "work".into(),
            path: "R&D".into(),
            excerpt: Some("内容摘要".into()),
        };
        store
            .append_message(
                &conv_id,
                &StoredChatMessage {
                    role: "user".into(),
                    content: "问题".into(),
                    reasoning: None,
                    context_files: vec![context],
                    created_at: 0,
                },
            )
            .unwrap();
        store
            .append_message(
                &conv_id,
                &StoredChatMessage {
                    role: "assistant".into(),
                    content: "回答".into(),
                    reasoning: Some("思考过程".into()),
                    context_files: vec![],
                    created_at: 0,
                },
            )
            .unwrap();

        let messages = store.get_messages(&conv_id).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "问题");
        assert_eq!(messages[0].context_files.len(), 1);
        assert_eq!(messages[0].context_files[0].item.name, "notes.md");
        assert_eq!(messages[0].context_files[0].account_name, "work");
        assert_eq!(messages[1].reasoning.as_deref(), Some("思考过程"));
        assert!(messages[0].created_at > 0, "timestamp must be filled in");
    }

    #[test]
    fn first_conversation_is_created_and_reused() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        let first = store.open_latest_or_create().unwrap();
        let second = store.open_latest_or_create().unwrap();
        assert_eq!(first, second, "latest conversation must be stable");
        assert!(store.list_conversations().unwrap().len() >= 1);
    }

    // Feature: ai-assistant, Property 9: untitled conversations auto-title
    #[test]
    fn auto_title_from_first_user_message() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        let conv_id = store.create_conversation("").unwrap();
        store.append_message(&conv_id, &user_msg("帮我看一下星星图是什么")).unwrap();

        let meta = &store.list_conversations().unwrap()[0];
        assert_eq!(meta.title, "帮我看一下星星图是什么");

        // Assistant messages never change the title.
        store
            .append_message(
                &conv_id,
                &StoredChatMessage {
                    role: "assistant".into(),
                    content: "完全不同的内容".into(),
                    reasoning: None,
                    context_files: vec![],
                    created_at: 0,
                },
            )
            .unwrap();
        assert_eq!(store.list_conversations().unwrap()[0].title, "帮我看一下星星图是什么");
    }

    #[test]
    fn delete_removes_messages_and_conversation() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        let conv_id = store.create_conversation("temp").unwrap();
        store.append_message(&conv_id, &user_msg("hi")).unwrap();
        store.delete_conversation(&conv_id).unwrap();

        assert!(store.open_conversation(&conv_id).unwrap().is_none());
        assert!(store.get_messages(&conv_id).unwrap().is_empty());
    }

    #[test]
    fn migrations_are_idempotent() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);
        store.open().unwrap();
        store.open().unwrap(); // second open must not fail or duplicate schema
        assert!(store.list_conversations().unwrap().is_empty());
    }

    #[test]
    fn chat_snapshot_round_trip_preserves_rows_and_ids() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("chat_history.db");
        let store = ChatHistoryStore::new(dir.path().to_path_buf());

        let conv_a = store.create_conversation("第一段对话").unwrap();
        store
            .append_message(&conv_a, &user_msg("你好"))
            .unwrap();
        store
            .append_message(
                &conv_a,
                &StoredChatMessage {
                    role: "assistant".into(),
                    content: "回答".into(),
                    reasoning: Some("推理".into()),
                    context_files: vec![],
                    created_at: 0,
                },
            )
            .unwrap();
        let conv_b = store.create_conversation("第二段对话").unwrap();
        store.append_message(&conv_b, &user_msg("第二条")).unwrap();

        // One memory plus one internal counter row.
        let memory = crate::store::memory::MemoryStore::new(dir.path().to_path_buf());
        memory.save("", "用户偏好深色主题", &conv_a).unwrap();
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute(
                "INSERT INTO memories (id, content, source_conversation_id, enabled, pinned, use_count, last_used_at, created_at, updated_at)
                 VALUES ('__counter_c1', '', '__counter', 1, 0, 7, 0, 100, 100)",
                [],
            )
            .unwrap();
        }
        let message_ids: Vec<i64> = {
            let conn = Connection::open(&db_path).unwrap();
            let mut stmt = conn.prepare("SELECT id FROM messages ORDER BY id").unwrap();
            let ids = stmt
                .query_map([], |r| r.get(0))
                .unwrap()
                .filter_map(Result::ok)
                .collect();
            ids
        };

        let snapshot = export_chat_history_snapshot(&db_path).unwrap();
        assert_eq!(snapshot.conversations.len(), 2);
        assert_eq!(snapshot.messages.len(), 3);
        assert_eq!(snapshot.memories.len(), 2);

        // Restore into a FRESH location (simulates another machine) and
        // verify rows, ids and ordering survive.
        let target_dir = TempDir::new().unwrap();
        let target_db = target_dir.path().join("chat_history.db");
        import_chat_history_snapshot(&target_db, &snapshot).unwrap();

        let restored_store = ChatHistoryStore::new(target_dir.path().to_path_buf());
        let restored_msgs = restored_store.get_messages(&conv_a).unwrap();
        assert_eq!(restored_msgs.len(), 2);
        assert_eq!(restored_msgs[0].content, "你好");
        assert_eq!(restored_msgs[1].reasoning.as_deref(), Some("推理"));
        let restored_metas = restored_store.list_conversations().unwrap();
        assert_eq!(restored_metas.len(), 2);

        // Message ids preserved exactly.
        let conn = Connection::open(&target_db).unwrap();
        let mut stmt = conn.prepare("SELECT id FROM messages ORDER BY id").unwrap();
        let restored_ids: Vec<i64> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(restored_ids, message_ids);

        // Counter row survives (extraction position kept).
        let counter: i64 = conn
            .query_row(
                "SELECT use_count FROM memories WHERE id = '__counter_c1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(counter, 7);

        // New messages after restore continue above the highest restored id.
        restored_store
            .append_message(&conv_a, &user_msg("恢复后新消息"))
            .unwrap();
        let conn = Connection::open(&target_db).unwrap();
        let new_id: i64 = conn
            .query_row(
                "SELECT id FROM messages WHERE content = '恢复后新消息'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            restored_ids.iter().all(|id| new_id > *id),
            "new id {} must exceed restored ids {:?}",
            new_id,
            restored_ids
        );
    }

    #[test]
    fn import_replaces_existing_rows() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("chat_history.db");
        let store = ChatHistoryStore::new(dir.path().to_path_buf());
        let conv = store.create_conversation("将被清空").unwrap();
        store.append_message(&conv, &user_msg("旧消息")).unwrap();

        // An empty snapshot must wipe existing rows.
        import_chat_history_snapshot(&db_path, &ChatHistorySnapshot::default()).unwrap();
        assert!(store.list_conversations().unwrap().is_empty());
    }
}
