use tauri::State;

use crate::errors::AppError;
use crate::store::chat_history::{
    ChatHistoryStore, ConversationDetail, ConversationMeta, StoredChatMessage,
};

/// List all conversations, most recently updated first.
#[tauri::command]
pub async fn chat_list_conversations(
    store: State<'_, ChatHistoryStore>,
) -> Result<Vec<ConversationMeta>, AppError> {
    store.list_conversations()
}

/// Open a conversation by id; `None` opens (or creates) the latest one.
#[tauri::command]
pub async fn chat_open_conversation(
    conversation_id: Option<String>,
    store: State<'_, ChatHistoryStore>,
) -> Result<ConversationDetail, AppError> {
    let id = match conversation_id {
        Some(id) => id,
        None => store.open_latest_or_create()?,
    };
    match store.open_conversation(&id)? {
        Some(detail) => Ok(detail),
        None => {
            let fallback = store.open_latest_or_create()?;
            let messages = store.get_messages(&fallback)?;
            Ok(ConversationDetail {
                id: fallback,
                messages,
            })
        }
    }
}

/// Start a new empty conversation; returns its id.
#[tauri::command]
pub async fn chat_new_conversation(
    store: State<'_, ChatHistoryStore>,
) -> Result<String, AppError> {
    store.create_conversation("")
}

/// Append a message to a conversation.
#[tauri::command]
pub async fn chat_append_message(
    conversation_id: String,
    message: StoredChatMessage,
    store: State<'_, ChatHistoryStore>,
) -> Result<(), AppError> {
    store.append_message(&conversation_id, &message)
}

/// Delete a conversation and its messages; unknown ids are ignored.
#[tauri::command]
pub async fn chat_delete_conversation(
    conversation_id: String,
    store: State<'_, ChatHistoryStore>,
) -> Result<(), AppError> {
    store.delete_conversation(&conversation_id)
}
