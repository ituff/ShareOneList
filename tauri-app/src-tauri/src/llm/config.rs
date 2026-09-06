// LLM provider configuration: llm.json persistence + keyring-backed API keys

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::errors::AppError;

const LLM_FILE_NAME: &str = "llm.json";
const KEYRING_SERVICE: &str = "shareonelist";
const API_KEY_PREFIX: &str = "llm_api_key_";

/// Wire protocol of the provider endpoint. Everything except Azure speaks the
/// OpenAI chat-completions dialect. Renamed explicitly to match the values
/// the frontend sends (`openai_compatible` / `azure_openai`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LlmProviderKind {
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible,
    #[serde(rename = "azure_openai")]
    AzureOpenAi,
}

/// Known provider presets. Values only pre-fill the config form; the user may
/// edit every field afterwards. Renamed explicitly to match the frontend's
/// kebab-case identifiers (`azure-openai`, not serde's derived `azure-open-ai`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LlmProviderPreset {
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "azure-openai")]
    AzureOpenAi,
    #[serde(rename = "deepseek")]
    Deepseek,
    #[serde(rename = "dashscope")]
    Dashscope,
    #[serde(rename = "moonshot")]
    Moonshot,
    #[serde(rename = "zhipu")]
    Zhipu,
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "custom")]
    Custom,
}

impl LlmProviderPreset {
    /// Default Base URL for the preset; empty for custom (user must fill in).
    pub fn default_base_url(&self) -> &'static str {
        match self {
            LlmProviderPreset::OpenAi => "https://api.openai.com/v1",
            LlmProviderPreset::AzureOpenAi => "https://<resource-name>.openai.azure.com",
            LlmProviderPreset::Deepseek => "https://api.deepseek.com/v1",
            LlmProviderPreset::Dashscope => {
                "https://dashscope.aliyuncs.com/compatible-mode/v1"
            }
            LlmProviderPreset::Moonshot => "https://api.moonshot.cn/v1",
            LlmProviderPreset::Zhipu => "https://open.bigmodel.cn/api/paas/v4",
            LlmProviderPreset::Ollama => "http://localhost:11434/v1",
            LlmProviderPreset::Custom => "",
        }
    }

    pub fn default_kind(&self) -> LlmProviderKind {
        match self {
            LlmProviderPreset::AzureOpenAi => LlmProviderKind::AzureOpenAi,
            _ => LlmProviderKind::OpenAiCompatible,
        }
    }

    /// Default default API version for Azure deployments.
    pub fn default_api_version(&self) -> Option<&'static str> {
        match self {
            LlmProviderPreset::AzureOpenAi => Some("2024-10-21"),
            _ => None,
        }
    }
}

/// A model exposed by a provider. `model_id` is the wire identifier
/// (Azure: deployment name); `display_name` is what the UI shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmModelConfig {
    pub id: String,
    pub model_id: String,
    pub display_name: String,
}

/// Reference to a specific model across all providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmModelRef {
    pub provider_id: String,
    pub model_id: String,
}

/// A configured model provider. The API key itself never lives here — only
/// `has_api_key`, populated when the config is loaded from disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmProviderConfig {
    pub id: String,
    pub name: String,
    pub preset: LlmProviderPreset,
    pub kind: LlmProviderKind,
    pub base_url: String,
    #[serde(default)]
    pub api_version: Option<String>,
    #[serde(default)]
    pub models: Vec<LlmModelConfig>,
    /// True when an API key exists in the keyring. Not deserialized from the
    /// file (the field is overwritten on load).
    #[serde(default)]
    pub has_api_key: bool,
}

/// Root of `llm.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfig {
    #[serde(default)]
    pub providers: Vec<LlmProviderConfig>,
    #[serde(default)]
    pub default_model: Option<LlmModelRef>,
}

impl LlmConfig {
    /// Look up a provider by id.
    pub fn provider(&self, provider_id: &str) -> Option<&LlmProviderConfig> {
        self.providers.iter().find(|p| p.id == provider_id)
    }

    /// True when the default reference points at an existing provider model.
    pub fn default_model_is_valid(&self) -> bool {
        match &self.default_model {
            None => true,
            Some(reference) => self
                .provider(&reference.provider_id)
                .is_some_and(|p| p.models.iter().any(|m| m.model_id == reference.model_id)),
        }
    }
}

/// Manages `llm.json` persistence and keyring-backed API keys.
pub struct LlmConfigManager {
    config_path: PathBuf,
}

impl LlmConfigManager {
    pub fn new(base_path: PathBuf) -> Self {
        Self {
            config_path: base_path.join(LLM_FILE_NAME),
        }
    }

    /// Load the LLM configuration, falling back to defaults when the file is
    /// missing or invalid. `has_api_key` is hydrated from the keyring.
    pub fn load(&self) -> LlmConfig {
        let mut config = match fs::read_to_string(&self.config_path) {
            Ok(content) => serde_json::from_str::<LlmConfig>(&content).unwrap_or_default(),
            Err(_) => LlmConfig::default(),
        };
        for provider in &mut config.providers {
            provider.has_api_key = self.load_api_key(&provider.id).is_some();
        }
        if !config.default_model_is_valid() {
            config.default_model = None;
        }
        config
    }

    /// Persist the configuration. The keyring is the source of truth for
    /// `has_api_key`, so the flag is recomputed rather than trusted from input.
    pub fn save(&self, config: &LlmConfig) -> Result<(), AppError> {
        if !config.default_model_is_valid() {
            return Err(AppError::Validation {
                message: "default model reference does not point to an existing model".into(),
                field: "defaultModel".into(),
            });
        }
        if let Some(parent) = self.config_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| AppError::Config {
                    message: e.to_string(),
                })?;
            }
        }
        let mut to_store = config.clone();
        for provider in &mut to_store.providers {
            provider.has_api_key = self.load_api_key(&provider.id).is_some();
        }
        let json = serde_json::to_string_pretty(&to_store).map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        fs::write(&self.config_path, json).map_err(|e| AppError::Config {
            message: e.to_string(),
        })?;
        Ok(())
    }

    fn keyring_entry(&self, provider_id: &str) -> Option<keyring::Entry> {
        keyring::Entry::new(KEYRING_SERVICE, &format!("{}{}", API_KEY_PREFIX, provider_id)).ok()
    }

    /// Store (or overwrite) the API key for a provider.
    pub fn store_api_key(&self, provider_id: &str, api_key: &str) -> Result<(), AppError> {
        let entry = self.keyring_entry(provider_id).ok_or_else(|| AppError::Config {
            message: "keyring unavailable".into(),
        })?;
        entry.set_password(api_key).map_err(|e| AppError::Config {
            message: format!("failed to store API key: {}", e),
        })
    }

    /// Load the full API key (backend use only; never sent to the frontend).
    pub fn load_api_key(&self, provider_id: &str) -> Option<String> {
        self.keyring_entry(provider_id)
            .and_then(|entry| entry.get_password().ok())
            .filter(|key| !key.is_empty())
    }

    /// Remove the stored API key; missing entries are fine.
    pub fn delete_api_key(&self, provider_id: &str) {
        if let Some(entry) = self.keyring_entry(provider_id) {
            let _ = entry.delete_credential();
        }
    }

    /// Masked key preview for the UI, e.g. `sk-…abcd`. None when unset.
    pub fn masked_api_key(&self, provider_id: &str) -> Option<String> {
        let key = self.load_api_key(provider_id)?;
        let tail: String = key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
        Some(format!("…{}", tail))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_manager(dir: &TempDir) -> LlmConfigManager {
        LlmConfigManager::new(dir.path().to_path_buf())
    }

    fn provider(id: &str, model_ids: &[&str]) -> LlmProviderConfig {
        LlmProviderConfig {
            id: id.to_string(),
            name: format!("Provider {}", id),
            preset: LlmProviderPreset::Custom,
            kind: LlmProviderKind::OpenAiCompatible,
            base_url: "https://example.com/v1".into(),
            api_version: None,
            models: model_ids
                .iter()
                .map(|m| LlmModelConfig {
                    id: format!("{}-{}", id, m),
                    model_id: m.to_string(),
                    display_name: m.to_string(),
                })
                .collect(),
            has_api_key: false,
        }
    }

    // Feature: ai-assistant, Property 1: LLM config round-trip preserves semantics
    #[test]
    fn save_and_load_round_trip() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);

        let config = LlmConfig {
            providers: vec![
                provider("p1", &["gpt-4o", "gpt-4o-mini"]),
                provider("p2", &["deepseek-chat"]),
            ],
            default_model: Some(LlmModelRef {
                provider_id: "p1".into(),
                model_id: "gpt-4o".into(),
            }),
        };
        mgr.save(&config).unwrap();
        let loaded = mgr.load();

        assert_eq!(loaded.providers.len(), 2);
        assert_eq!(loaded.providers[0].models.len(), 2);
        assert_eq!(loaded.providers[0].models[1].model_id, "gpt-4o-mini");
        assert_eq!(loaded.providers[1].models[0].model_id, "deepseek-chat");
        assert_eq!(
            loaded.default_model,
            Some(LlmModelRef {
                provider_id: "p1".into(),
                model_id: "gpt-4o".into(),
            })
        );
    }

    #[test]
    fn load_returns_default_when_file_missing_or_invalid() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);
        assert!(mgr.load().providers.is_empty());

        std::fs::write(dir.path().join("llm.json"), "{{{ not json").unwrap();
        assert!(mgr.load().providers.is_empty());
    }

    // Feature: ai-assistant, Property 2: default model reference invariant
    #[test]
    fn save_rejects_default_referring_to_missing_model() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);

        let config = LlmConfig {
            providers: vec![provider("p1", &["m1"])],
            default_model: Some(LlmModelRef {
                provider_id: "p1".into(),
                model_id: "nope".into(),
            }),
        };
        assert!(mgr.save(&config).is_err());

        let dangling_provider = LlmConfig {
            providers: vec![provider("p1", &["m1"])],
            default_model: Some(LlmModelRef {
                provider_id: "ghost".into(),
                model_id: "m1".into(),
            }),
        };
        assert!(mgr.save(&dangling_provider).is_err());
    }

    // Feature: ai-assistant, Property 3: deleting the default provider is rejected
    #[test]
    fn deleting_default_provider_is_rejected_and_config_unchanged() {
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);

        let config = LlmConfig {
            providers: vec![provider("p1", &["m1"]), provider("p2", &["m2"])],
            default_model: Some(LlmModelRef {
                provider_id: "p1".into(),
                model_id: "m1".into(),
            }),
        };
        mgr.save(&config).unwrap();

        // The delete validation lives in commands.rs; here we verify the
        // invariant check catches the removal.
        let mut after_delete = config.clone();
        after_delete.providers.retain(|p| p.id != "p1");
        assert!(mgr.save(&after_delete).is_err());

        let reloaded = mgr.load();
        assert_eq!(reloaded.providers.len(), 2, "config must be unchanged");
        assert_eq!(reloaded.default_model.as_ref().unwrap().provider_id, "p1");
    }

    // Feature: ai-assistant, Property 4: keyring entry names are deterministic
    #[test]
    fn api_key_operations_are_noop_safe_without_keyring_backend() {
        // On CI without a usable keyring backend these must not panic; on
        // developer machines they exercise the real path.
        let dir = TempDir::new().unwrap();
        let mgr = make_manager(&dir);
        let key = mgr.load_api_key("nonexistent-provider");
        assert!(key.is_none());
        mgr.delete_api_key("nonexistent-provider"); // must not panic
        assert!(mgr.masked_api_key("nonexistent-provider").is_none());
    }

    #[test]
    fn preset_defaults_are_consistent() {
        assert_eq!(
            LlmProviderPreset::AzureOpenAi.default_kind(),
            LlmProviderKind::AzureOpenAi
        );
        assert!(LlmProviderPreset::AzureOpenAi.default_api_version().is_some());
        for preset in [
            LlmProviderPreset::OpenAi,
            LlmProviderPreset::Deepseek,
            LlmProviderPreset::Dashscope,
            LlmProviderPreset::Moonshot,
            LlmProviderPreset::Zhipu,
            LlmProviderPreset::Ollama,
        ] {
            assert_eq!(preset.default_kind(), LlmProviderKind::OpenAiCompatible);
            assert!(!preset.default_base_url().is_empty());
        }
        assert_eq!(LlmProviderPreset::Custom.default_base_url(), "");
    }

    // Feature: ai-assistant, Property 6: frontend enum strings deserialize
    #[test]
    fn frontend_enum_values_deserialize() {
        // The exact literals src/lib/types.ts sends must be accepted.
        for kind_str in ["openai_compatible", "azure_openai"] {
            let kind: LlmProviderKind = serde_json::from_str(&format!("\"{}\"", kind_str)).unwrap();
            let serialized = serde_json::to_string(&kind).unwrap();
            assert_eq!(serialized, format!("\"{}\"", kind_str), "serialization must round-trip");
        }
        for preset_str in [
            "openai",
            "azure-openai",
            "deepseek",
            "dashscope",
            "moonshot",
            "zhipu",
            "ollama",
            "custom",
        ] {
            let preset: LlmProviderPreset =
                serde_json::from_str(&format!("\"{}\"", preset_str)).unwrap();
            let serialized = serde_json::to_string(&preset).unwrap();
            assert_eq!(serialized, format!("\"{}\"", preset_str), "serialization must round-trip");
        }
    }

    // Feature: ai-assistant, Property 6 (continued): full provider payload from
    // the frontend deserializes without errors
    #[test]
    fn frontend_provider_payload_deserializes() {
        let payload = r#"{
            "id": "",
            "name": "阿里百炼",
            "preset": "dashscope",
            "kind": "openai_compatible",
            "baseUrl": "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "apiVersion": null,
            "models": [{"id": "model-1", "modelId": "qwen-turbo", "displayName": "qwen-turbo"}],
            "hasApiKey": false
        }"#;
        let provider: LlmProviderConfig = serde_json::from_str(payload).unwrap();
        assert_eq!(provider.preset, LlmProviderPreset::Dashscope);
        assert_eq!(provider.kind, LlmProviderKind::OpenAiCompatible);
        assert_eq!(provider.models[0].model_id, "qwen-turbo");
    }
}
