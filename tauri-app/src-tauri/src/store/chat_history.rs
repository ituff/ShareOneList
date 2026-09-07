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
}
