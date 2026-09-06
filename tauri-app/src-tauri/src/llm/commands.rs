use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::State;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::errors::AppError;
use crate::llm::client::{self, LlmChatMessage, LlmContextFile};
use crate::llm::config::{LlmConfig, LlmConfigManager, LlmModelRef, LlmProviderConfig};

/// Live registry of in-flight chat requests for cancellation.
pub type ChatRegistry = Mutex<HashMap<String, CancellationToken>>;

/// LLM config plus masked key previews for the UI.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfigSnapshot {
    pub config: LlmConfig,
    pub masked_keys: HashMap<String, String>,
}

fn now_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("{}_{}", prefix, nanos)
}

fn snapshot(manager: &LlmConfigManager, config: LlmConfig) -> LlmConfigSnapshot {
    let masked_keys = config
        .providers
        .iter()
        .filter_map(|p| manager.masked_api_key(&p.id).map(|m| (p.id.clone(), m)))
        .collect();
    LlmConfigSnapshot {
        config,
        masked_keys,
    }
}

fn validate_provider(provider: &LlmProviderConfig) -> Result<(), AppError> {
    if provider.name.trim().is_empty() {
        return Err(AppError::Validation {
            message: "provider name is required".into(),
            field: "name".into(),
        });
    }
    if provider.base_url.trim().is_empty() {
        return Err(AppError::Validation {
            message: "base URL is required".into(),
            field: "baseUrl".into(),
        });
    }
    if provider.models.is_empty() {
        return Err(AppError::Validation {
            message: "at least one model is required".into(),
            field: "models".into(),
        });
    }
    if provider.models.iter().any(|m| m.model_id.trim().is_empty()) {
        return Err(AppError::Validation {
            message: "model id must not be empty".into(),
            field: "models".into(),
        });
    }
    Ok(())
}

/// Read the LLM configuration with masked key previews.
#[tauri::command]
pub fn get_llm_config(manager: State<'_, LlmConfigManager>) -> LlmConfigSnapshot {
    let config = manager.load();
    snapshot(&manager, config)
}

/// Create or update a provider. `api_key` semantics: None keeps the stored
/// key unchanged, empty string clears it, a non-empty value stores a new key.
#[tauri::command]
pub fn save_llm_provider(
    provider: LlmProviderConfig,
    api_key: Option<String>,
    manager: State<'_, LlmConfigManager>,
) -> Result<LlmConfigSnapshot, AppError> {
    validate_provider(&provider)?;

    let mut config = manager.load();
    let is_new = provider.id.is_empty() || config.provider(&provider.id).is_none();
    let mut provider = provider;
    if is_new {
        provider.id = now_id("prov");
    }

    match api_key.as_deref() {
        None => {}
        Some("") => manager.delete_api_key(&provider.id),
        Some(key) => manager.store_api_key(&provider.id, key)?,
    }

    config.providers.retain(|p| p.id != provider.id);
    config.providers.push(provider);

    // First usable provider becomes the default automatically.
    if config.default_model.is_none() {
        if let Some(first_model) = config.providers.last().and_then(|p| p.models.first()) {
            config.default_model = Some(LlmModelRef {
                provider_id: config.providers.last().unwrap().id.clone(),
                model_id: first_model.model_id.clone(),
            });
        }
    }

    manager.save(&config)?;
    Ok(snapshot(&manager, config))
}

/// Delete a provider and its stored key. Refuses when it holds the default
/// model — the user must pick another default first.
#[tauri::command]
pub fn delete_llm_provider(
    provider_id: String,
    manager: State<'_, LlmConfigManager>,
) -> Result<LlmConfigSnapshot, AppError> {
    let mut config = manager.load();
    if config
        .default_model
        .as_ref()
        .is_some_and(|m| m.provider_id == provider_id)
    {
        return Err(AppError::Validation {
            message: "cannot delete the provider that holds the default model".into(),
            field: "defaultModel".into(),
        });
    }
    config.providers.retain(|p| p.id != provider_id);
    manager.save(&config)?;
    manager.delete_api_key(&provider_id);
    Ok(snapshot(&manager, config))
}

/// Set the default model after verifying the reference exists.
#[tauri::command]
pub fn set_default_model(
    provider_id: String,
    model_id: String,
    manager: State<'_, LlmConfigManager>,
) -> Result<LlmConfigSnapshot, AppError> {
    let mut config = manager.load();
    let provider = config.provider(&provider_id).ok_or(AppError::Validation {
        message: "provider not found".into(),
        field: "providerId".into(),
    })?;
    if !provider.models.iter().any(|m| m.model_id == model_id) {
        return Err(AppError::Validation {
            message: "model not found on provider".into(),
            field: "modelId".into(),
        });
    }
    config.default_model = Some(LlmModelRef {
        provider_id,
        model_id,
    });
    manager.save(&config)?;
    Ok(snapshot(&manager, config))
}

/// Send a minimal chat request against the provider to verify connectivity
/// and credentials. Uses the supplied key when present, else the stored one.
#[tauri::command]
pub async fn test_llm_connection(
    provider: LlmProviderConfig,
    api_key: Option<String>,
    manager: State<'_, LlmConfigManager>,
) -> Result<(), AppError> {
    let stored_key = manager.load_api_key(&provider.id).unwrap_or_default();
    let key = api_key.filter(|k| !k.is_empty()).unwrap_or(stored_key);
    if key.is_empty() {
        return Err(AppError::Validation {
            message: "API key is required".into(),
            field: "apiKey".into(),
        });
    }
    client::test_connection(&provider, &key).await
}

/// Fetch the model ids the provider API reports as available.
#[tauri::command]
pub async fn list_llm_models(
    provider: LlmProviderConfig,
    api_key: Option<String>,
    manager: State<'_, LlmConfigManager>,
) -> Result<Vec<String>, AppError> {
    let stored_key = manager.load_api_key(&provider.id).unwrap_or_default();
    let key = api_key.filter(|k| !k.is_empty()).unwrap_or(stored_key);
    if key.is_empty() {
        return Err(AppError::Validation {
            message: "API key is required".into(),
            field: "apiKey".into(),
        });
    }
    client::fetch_models(&provider, &key).await
}

/// Start a streaming chat request. Returns a request id; deltas arrive as
/// `llm-chat-event` events until a "done" or "error" payload.
#[tauri::command]
pub async fn llm_chat(
    provider_id: String,
    model_id: String,
    messages: Vec<LlmChatMessage>,
    // Client-generated id so the frontend can filter events from the very
    // first delta (which may arrive before the invoke promise resolves).
    request_id: Option<String>,
    // Files found by searching the user's cloud for the current question;
    // woven into the backend-owned system prompt.
    context_files: Option<Vec<LlmContextFile>>,
    // Reasoning effort for reasoning models ("low" | "medium" | "high").
    reasoning_effort: Option<String>,
    manager: State<'_, LlmConfigManager>,
    registry: State<'_, ChatRegistry>,
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    let mut messages = messages;
    if messages.is_empty() {
        return Err(AppError::Validation {
            message: "chat messages must not be empty".into(),
            field: "messages".into(),
        });
    }
    let config = manager.load();
    let provider = config.provider(&provider_id).cloned().ok_or(AppError::Validation {
        message: "provider not found".into(),
        field: "providerId".into(),
    })?;
    if !provider.models.iter().any(|m| m.model_id == model_id) {
        return Err(AppError::Validation {
            message: "model not found on provider".into(),
            field: "modelId".into(),
        });
    }
    let api_key = manager
        .load_api_key(&provider_id)
        .ok_or(AppError::Validation {
            message: "API key is not configured for this provider".into(),
            field: "apiKey".into(),
        })?;

    let request_id = match request_id {
        Some(id) if !id.trim().is_empty() => id,
        _ => now_id("req"),
    };
    let cancel = CancellationToken::new();
    registry.lock().await.insert(request_id.clone(), cancel.clone());

    // The spawned task needs its own provider copy with the selected model
    // promoted to the front so adapters resolve the right model.
    let mut provider = provider;
    if let Some(index) = provider.models.iter().position(|m| m.model_id == model_id) {
        provider.models.swap(0, index);
    }

    // The system prompt is backend-owned: strip any client-provided system
    // messages and prepend the grounding prompt with the cloud file context.
    let system_prompt = client::build_system_prompt(&context_files.unwrap_or_default());
    messages.retain(|m| m.role != "system");
    messages.insert(
        0,
        LlmChatMessage {
            role: "system".into(),
            content: system_prompt,
        },
    );

    let spawn_request_id = request_id.clone();
    tauri::async_runtime::spawn(async move {
        client::stream_chat(
            app_handle.clone(),
            spawn_request_id.clone(),
            provider,
            api_key,
            messages,
            reasoning_effort.filter(|e| ["low", "medium", "high"].contains(&e.as_str())),
            cancel,
        )
        .await;
        use tauri::Manager;
        app_handle
            .state::<ChatRegistry>()
            .lock()
            .await
            .remove(&spawn_request_id);
    });

    Ok(request_id)
}

/// Cancel an in-flight chat request; unknown ids are ignored.
#[tauri::command]
pub async fn llm_chat_cancel(
    request_id: String,
    registry: State<'_, ChatRegistry>,
) -> Result<(), AppError> {
    if let Some(cancel) = registry.lock().await.remove(&request_id) {
        cancel.cancel();
    }
    Ok(())
}
