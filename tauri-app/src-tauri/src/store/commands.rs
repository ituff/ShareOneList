use std::sync::Arc;

use tauri::{Emitter, State};

use crate::errors::AppError;
use crate::llm::commands::ExtractLock;
use crate::llm::config::LlmConfigManager;
use crate::llm::memory;
use crate::store::chat_history::{
    ChatHistoryStore, ConversationDetail, ConversationMeta, StoredChatMessage,
};
use crate::store::memory::{MemoryEntry, MemoryStore};

/// List all conversations, most recently updated first.
#[tauri::command]
pub async fn chat_list_conversations(
    store: State<'_, Arc<ChatHistoryStore>>,
) -> Result<Vec<ConversationMeta>, AppError> {
    store.list_conversations()
}

/// Open a conversation by id; `None` opens (or creates) the latest one.
#[tauri::command]
pub async fn chat_open_conversation(
    conversation_id: Option<String>,
    store: State<'_, Arc<ChatHistoryStore>>,
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
    store: State<'_, Arc<ChatHistoryStore>>,
) -> Result<String, AppError> {
    store.create_conversation("")
}

/// Append a message to a conversation.
#[tauri::command]
pub async fn chat_append_message(
    conversation_id: String,
    message: StoredChatMessage,
    store: State<'_, Arc<ChatHistoryStore>>,
    memory: State<'_, Arc<MemoryStore>>,
    manager: State<'_, LlmConfigManager>,
    extract_lock: State<'_, ExtractLock>,
    app_handle: tauri::AppHandle,
) -> Result<(), AppError> {
    store.append_message(&conversation_id, &message)?;

    // Background memory extraction, best-effort (requirements 1.3).
    if message.role != "user" {
        return Ok(());
    }
    let config = manager.load();
    if !config.memory.enabled {
        return Ok(());
    }
    let every = (config.memory.extract_every.max(1)) as usize;
    let count = memory
        .user_messages_since_extract(&conversation_id)
        .unwrap_or(0)
        + 1;
    if count < every {
        let _ = memory.set_user_messages_since_extract(&conversation_id, count);
        return Ok(());
    }
    let _ = memory.set_user_messages_since_extract(&conversation_id, 0);

    // Resolve the default model up front; without one extraction is skipped.
    let Some(default_ref) = config.default_model.clone() else {
        return Ok(());
    };
    let Some(provider) = config.provider(&default_ref.provider_id).cloned() else {
        return Ok(());
    };
    let Some(api_key) = manager.load_api_key(&default_ref.provider_id) else {
        return Ok(());
    };
    let memory: Arc<MemoryStore> = memory.inner().clone();
    let store: Arc<ChatHistoryStore> = store.inner().clone();
    let lock: ExtractLock = extract_lock.inner().clone();
    let app = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        // Single-flight: skip when another extraction is running.
        let Ok(_guard) = lock.try_lock() else {
            return;
        };
        let _ = app.emit("memory-event", serde_json::json!({ "kind": "extracting" }));
        match memory::extract_and_merge(&memory, &provider, &api_key, &conversation_id).await {
            Ok(count) => {
                let _ = app.emit(
                    "memory-event",
                    serde_json::json!({ "kind": "extracted", "count": count }),
                );
            }
            Err(e) => eprintln!("[memory] extraction skipped: {e}"),
        }
        let _ = store; // kept for future windowing refinements
    });
    Ok(())
}

/// Delete a conversation and its messages; unknown ids are ignored.
#[tauri::command]
pub async fn chat_delete_conversation(
    conversation_id: String,
    store: State<'_, Arc<ChatHistoryStore>>,
) -> Result<(), AppError> {
    store.delete_conversation(&conversation_id)
}

// ─── User memory management ─────────────────────────────────────────────────

#[tauri::command]
pub async fn memory_list(
    memory: State<'_, Arc<MemoryStore>>,
) -> Result<Vec<MemoryEntry>, AppError> {
    memory.list()
}

#[tauri::command]
pub async fn memory_save(
    id: String,
    content: String,
    conversation_id: String,
    memory: State<'_, Arc<MemoryStore>>,
) -> Result<String, AppError> {
    memory.save(&id, &content, &conversation_id)
}

#[tauri::command]
pub async fn memory_delete(
    id: String,
    memory: State<'_, Arc<MemoryStore>>,
) -> Result<(), AppError> {
    memory.delete(&id)
}

#[tauri::command]
pub async fn memory_set_enabled(
    id: String,
    enabled: bool,
    memory: State<'_, Arc<MemoryStore>>,
) -> Result<(), AppError> {
    memory.set_enabled(&id, enabled)
}

#[tauri::command]
pub async fn memory_set_pinned(
    id: String,
    pinned: bool,
    memory: State<'_, Arc<MemoryStore>>,
) -> Result<(), AppError> {
    memory.set_pinned(&id, pinned)
}

#[tauri::command]
pub async fn memory_search_and_delete(
    keyword: String,
    memory: State<'_, Arc<MemoryStore>>,
) -> Result<usize, AppError> {
    memory.search_and_delete(&keyword)
}

/// Update the memory feature settings (persisted in llm.json).
#[tauri::command]
pub async fn memory_set_config(
    enabled: bool,
    extract_every: u32,
    manager: State<'_, LlmConfigManager>,
) -> Result<(), AppError> {
    let mut config = manager.load();
    config.memory.enabled = enabled;
    config.memory.extract_every = extract_every.clamp(1, 100);
    manager.save(&config)
}

/// Manual extraction trigger (settings page / debugging). Uses the default
/// model; respects the single-flight lock.
#[tauri::command]
pub async fn memory_extract_now(
    conversation_id: String,
    memory: State<'_, Arc<MemoryStore>>,
    manager: State<'_, LlmConfigManager>,
    extract_lock: State<'_, ExtractLock>,
) -> Result<usize, AppError> {
    let config = manager.load();
    if !config.memory.enabled {
        return Err(AppError::Validation {
            message: "memory feature is disabled".into(),
            field: "memory".into(),
        });
    }
    let default_ref = config.default_model.clone().ok_or(AppError::Validation {
        message: "no default model configured".into(),
        field: "defaultModel".into(),
    })?;
    let provider = config
        .provider(&default_ref.provider_id)
        .cloned()
        .ok_or(AppError::Validation {
            message: "default provider not found".into(),
            field: "defaultModel".into(),
        })?;
    let api_key = manager
        .load_api_key(&default_ref.provider_id)
        .ok_or(AppError::Validation {
            message: "API key is not configured for the default provider".into(),
            field: "apiKey".into(),
        })?;
    let Ok(_guard) = extract_lock.try_lock() else {
        return Err(AppError::Config {
            message: "another extraction is already running".into(),
        });
    };
    memory::extract_and_merge(memory.inner(), &provider, &api_key, &conversation_id)
        .await
        .map_err(|e| AppError::Config {
            message: format!("extraction failed: {e}"),
        })
}
