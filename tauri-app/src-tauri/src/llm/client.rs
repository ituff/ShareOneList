// LLM chat client: provider adapters, connection testing, and SSE streaming

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio_util::sync::CancellationToken;

use crate::errors::AppError;
use crate::llm::config::{LlmProviderConfig, LlmProviderKind};

/// Tauri event name for streaming chat updates.
pub const LLM_CHAT_EVENT: &str = "llm-chat-event";

/// A single conversation message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmChatMessage {
    pub role: String, // "system" | "user" | "assistant"
    pub content: String,
}

/// A cloud file surfaced to the model as context for the current question,
/// found by searching the user's logged-in OneDrive / SharePoint drives.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmContextFile {
    pub name: String,
    pub path: String,
    pub web_url: String,
    pub account_name: String,
    /// Truncated text content of the file, when it is a small text-based
    /// document; None for name-only hits (folders, binaries, oversized files).
    #[serde(default)]
    pub excerpt: Option<String>,
}

/// The assistant's system prompt. Encodes the grounding rule: answer from the
/// user's cloud files first; only with the user's consent fall back to
/// internet / public knowledge.
pub fn build_system_prompt(context_files: &[LlmContextFile]) -> String {
    let mut prompt = String::from(
        "You are the AI assistant inside ShareOneList, a OneDrive / SharePoint file \
manager. The user's documents live in their own Microsoft 365 cloud (OneDrive and \
SharePoint sites, possibly across Global and 21Vianet clouds).\n\n\
Rules:\n\
1. Ground your answers in the user's cloud files first. Files found in the user's \
cloud for the current question are listed below; rely on them when relevant and say \
which files your answer is based on. Files with an excerpt provide their (possibly \
truncated) text content — quote or summarize from it; files without one are known \
only by name and path.\n\
2. If the listed files and the conversation do not contain the needed information, \
do NOT silently answer from general knowledge. Ask the user whether to answer from \
internet / public knowledge instead, and clearly label such an answer as NOT based \
on their cloud files.\n\
3. Never invent file contents, file names, or paths. If no relevant files exist, \
say so plainly.\n\
4. Answer in the language the user writes in.\n\n\
Files found in the user's cloud for the current question:\n",
    );
    if context_files.is_empty() {
        prompt.push_str("(none)\n");
    } else {
        for file in context_files {
            prompt.push_str(&format!(
                "- {} — {} [account: {}] ({})\n",
                file.name,
                if file.path.is_empty() { "/" } else { &file.path },
                file.account_name,
                file.web_url
            ));
            if let Some(excerpt) = &file.excerpt {
                prompt.push_str(&format!("  excerpt: \"{}\"\n", excerpt));
            }
        }
    }
    prompt
}

/// Payload pushed to the frontend for a streaming chat request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmChatEvent {
    pub request_id: String,
    /// "delta" (partial content) | "done" | "error"
    pub kind: String,
    pub delta: Option<String>,
    pub message: Option<String>,
}

/// Request body accepted by the chat-completions wire format (OpenAI dialect).
#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [LlmChatMessage],
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    /// Reasoning effort for reasoning models ("low" | "medium" | "high");
    /// providers that don't know the field are expected to ignore it.
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
}

/// Minimal delta shape parsed out of the SSE `data:` lines.
#[derive(Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: StreamDelta,
}

#[derive(Deserialize, Default)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    /// Reasoning models (qwen3, deepseek-r1, …) stream their chain of thought
    /// in this field alongside `content`.
    #[serde(default)]
    reasoning_content: Option<String>,
}

/// Parsed content of one SSE `data:` line.
#[derive(Debug, Default, PartialEq)]
struct ParsedLine {
    content: Option<String>,
    reasoning: Option<String>,
    done: bool,
}

/// Computes the chat-completions endpoint URL for a provider.
///
/// OpenAI-compatible: `{base}/chat/completions`.
/// Azure: `{base}/openai/deployments/{model_id}/chat/completions?api-version={v}`.
pub fn chat_url(provider: &LlmProviderConfig, model_id: &str) -> Result<String, AppError> {
    let base = provider.base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(AppError::Validation {
            message: "provider base URL is empty".into(),
            field: "baseUrl".into(),
        });
    }
    if model_id.is_empty() {
        return Err(AppError::Validation {
            message: "model id is empty".into(),
            field: "modelId".into(),
        });
    }
    match provider.kind {
        LlmProviderKind::OpenAiCompatible => Ok(format!("{}/chat/completions", base)),
        LlmProviderKind::AzureOpenAi => {
            let version = provider.api_version.as_deref().unwrap_or("2024-10-21");
            // For Azure the model id doubles as the deployment name.
            Ok(format!(
                "{}/openai/deployments/{}/chat/completions?api-version={}",
                base, model_id, version
            ))
        }
    }
}

/// Auth headers for a provider's wire protocol.
pub fn auth_headers(api_key: &str) -> Vec<(String, String)> {
    // Azure also accepts the api-key header via Bearer on newer api versions,
    // but the explicit header is the documented contract for both.
    vec![("Authorization".into(), format!("Bearer {}", api_key))]
}

fn azure_auth_headers(api_key: &str) -> Vec<(String, String)> {
    vec![("api-key".into(), api_key.into())]
}

fn http_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Network {
            message: e.to_string(),
            retryable: false,
        })
}

/// Sends a minimal chat request to verify the endpoint, key, and model.
/// Distinguishes network-unreachable from auth/API failures in the error.
pub async fn test_connection(provider: &LlmProviderConfig, api_key: &str) -> Result<(), AppError> {
    let model = provider
        .models
        .first()
        .map(|m| m.model_id.clone())
        .unwrap_or_default();
    if model.is_empty() {
        return Err(AppError::Validation {
            message: "provider has no model configured".into(),
            field: "models".into(),
        });
    }
    let url = chat_url(provider, &model)?;
    let client = http_client()?;
    let body = ChatRequest {
        model: &model,
        messages: &[LlmChatMessage {
            role: "user".into(),
            content: "ping".into(),
        }],
        stream: false,
        max_tokens: Some(1),
        reasoning_effort: None,
    };

    let mut request = client.post(&url).json(&body);
    for (name, value) in headers_for(provider, api_key) {
        request = request.header(name, value);
    }

    let response = request.send().await.map_err(|e| {
        if e.is_connect() || e.is_timeout() {
            AppError::Network {
                message: format!("cannot reach {}: {}", provider.base_url, e),
                retryable: true,
            }
        } else {
            AppError::Network {
                message: e.to_string(),
                retryable: false,
            }
        }
    })?;

    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let snippet = response.text().await.unwrap_or_default();
    let snippet: String = snippet.chars().take(300).collect();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        Err(AppError::Network {
            message: format!("authentication failed (HTTP {}): {}", status.as_u16(), snippet),
            retryable: false,
        })
    } else {
        Err(AppError::GraphApi {
            message: format!("provider returned HTTP {}: {}", status.as_u16(), snippet),
            status_code: status.as_u16(),
        })
    }
}

fn headers_for(provider: &LlmProviderConfig, api_key: &str) -> Vec<(String, String)> {
    match provider.kind {
        LlmProviderKind::OpenAiCompatible => auth_headers(api_key),
        LlmProviderKind::AzureOpenAi => azure_auth_headers(api_key),
    }
}

/// Minimal shape of the `GET {base}/models` response.
#[derive(Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<ModelsEntry>,
}

#[derive(Deserialize)]
struct ModelsEntry {
    id: String,
}

/// Lists the models the provider API reports as available. Only meaningful
/// for OpenAI-compatible endpoints; Azure has no equivalent on the data plane.
pub async fn fetch_models(
    provider: &LlmProviderConfig,
    api_key: &str,
) -> Result<Vec<String>, AppError> {
    if provider.kind == LlmProviderKind::AzureOpenAi {
        return Err(AppError::Validation {
            message: "Azure OpenAI does not expose a model list; enter deployment names manually"
                .into(),
            field: "models".into(),
        });
    }
    let base = provider.base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(AppError::Validation {
            message: "provider base URL is empty".into(),
            field: "baseUrl".into(),
        });
    }
    let client = http_client()?;
    let mut request = client.get(format!("{}/models", base));
    for (name, value) in headers_for(provider, api_key) {
        request = request.header(name, value);
    }

    let response = request.send().await.map_err(|e| {
        if e.is_connect() || e.is_timeout() {
            AppError::Network {
                message: format!("cannot reach {}: {}", provider.base_url, e),
                retryable: true,
            }
        } else {
            AppError::Network {
                message: e.to_string(),
                retryable: false,
            }
        }
    })?;

    let status = response.status();
    if !status.is_success() {
        let snippet = response.text().await.unwrap_or_default();
        let snippet: String = snippet.chars().take(300).collect();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(AppError::Network {
                message: format!("authentication failed (HTTP {}): {}", status.as_u16(), snippet),
                retryable: false,
            });
        }
        return Err(AppError::GraphApi {
            message: format!("provider returned HTTP {}: {}", status.as_u16(), snippet),
            status_code: status.as_u16(),
        });
    }

    let models: ModelsResponse = response.json().await.map_err(|e| AppError::GraphApi {
        message: format!("unexpected /models response format: {}", e),
        status_code: status.as_u16(),
    })?;
    let mut ids: Vec<String> = models.data.into_iter().map(|m| m.id).collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Runs a streaming chat request, invoking `on_event` for each delta and a
/// final "done" or "error" event. Decoupled from Tauri so it can be tested.
pub async fn stream_chat_events(
    provider: &LlmProviderConfig,
    api_key: &str,
    messages: &[LlmChatMessage],
    reasoning_effort: Option<&str>,
    cancel: &CancellationToken,
    mut on_event: impl FnMut(LlmChatEvent),
) {
    let request_id = format!("req_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default());
    let result = run_stream(
        provider,
        api_key,
        messages,
        reasoning_effort,
        cancel,
        &request_id,
        &mut on_event,
    )
    .await;
    match result {
        Ok(()) => on_event(LlmChatEvent {
            request_id,
            kind: "done".into(),
            delta: None,
            message: None,
        }),
        Err(message) => on_event(LlmChatEvent {
            request_id,
            kind: "error".into(),
            delta: None,
            message: Some(message),
        }),
    }
}

/// Runs a streaming chat request, emitting `LLM_CHAT_EVENT` events to the
/// frontend as deltas arrive. Returns when the stream ends, errors, or the
/// cancellation token fires.
pub async fn stream_chat(
    app_handle: tauri::AppHandle,
    request_id: String,
    provider: LlmProviderConfig,
    api_key: String,
    messages: Vec<LlmChatMessage>,
    reasoning_effort: Option<String>,
    cancel: CancellationToken,
) {
    stream_chat_events(
        &provider,
        &api_key,
        &messages,
        reasoning_effort.as_deref(),
        &cancel,
        |event| {
            let _ = app_handle.emit(
                LLM_CHAT_EVENT,
                LlmChatEvent {
                    request_id: request_id.clone(),
                    ..event
                },
            );
        },
    )
    .await;
}

async fn run_stream(
    provider: &LlmProviderConfig,
    api_key: &str,
    messages: &[LlmChatMessage],
    reasoning_effort: Option<&str>,
    cancel: &CancellationToken,
    request_id: &str,
    mut on_event: &mut impl FnMut(LlmChatEvent),
) -> Result<(), String> {
    let model = provider
        .models
        .first()
        .map(|m| m.model_id.clone())
        .unwrap_or_default();
    let url = chat_url(provider, &model).map_err(|e| e.to_string())?;
    let client = http_client().map_err(|e| e.to_string())?;

    let mut request = client
        .post(&url)
        .json(&ChatRequest {
            model: &model,
            messages,
            stream: true,
            max_tokens: None,
            reasoning_effort: reasoning_effort.map(|e| e.to_string()),
        });
    for (name, value) in headers_for(provider, api_key) {
        request = request.header(name, value);
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("request failed: {}", e))?;
    let status = response.status();
    if !status.is_success() {
        let snippet = response.text().await.unwrap_or_default();
        let snippet: String = snippet.chars().take(300).collect();
        return Err(format!("provider returned HTTP {}: {}", status.as_u16(), snippet));
    }

    // Read the byte stream line by line, forwarding `data:` payloads as deltas.
    // SSE allows events split across chunk boundaries, so a persistent line
    // buffer is required.
    let mut stream = response.bytes_stream();
    let mut line_buffer: Vec<u8> = Vec::new();

    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            item = stream.next() => item,
        };

        match chunk {
            Some(Ok(bytes)) => {
                for line in drain_sse_deltas(&mut line_buffer, &bytes, false) {
                    emit_parsed(request_id, &line, &mut on_event);
                }
            }
            Some(Err(e)) => return Err(format!("stream error: {}", e)),
            None => {
                // Stream ended; flush any trailing buffered line.
                for line in drain_sse_deltas(&mut line_buffer, &[], true) {
                    emit_parsed(request_id, &line, &mut on_event);
                }
                return Ok(());
            }
        }
    }
}

/// Emits the content / reasoning deltas of one parsed SSE line.
fn emit_parsed(
    request_id: &str,
    line: &ParsedLine,
    on_event: &mut dyn FnMut(LlmChatEvent),
) {
    if let Some(delta) = &line.content {
        on_event(LlmChatEvent {
            request_id: request_id.to_string(),
            kind: "delta".into(),
            delta: Some(delta.clone()),
            message: None,
        });
    }
    if let Some(reasoning) = &line.reasoning {
        on_event(LlmChatEvent {
            request_id: request_id.to_string(),
            kind: "reasoning".into(),
            delta: Some(reasoning.clone()),
            message: None,
        });
    }
}

/// Feeds newly arrived bytes into the line buffer and returns every complete
/// SSE `data:` line. With `flush`, the remaining partial line is also
/// processed (end of stream).
fn drain_sse_deltas(buffer: &mut Vec<u8>, incoming: &[u8], flush: bool) -> Vec<ParsedLine> {
    buffer.extend_from_slice(incoming);
    let mut lines = Vec::new();
    while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = buffer.drain(..=pos).collect();
        if let Some(parsed) = decode_sse_line(&line[..line.len() - 1]) {
            lines.push(parsed);
        }
    }
    if flush && !buffer.is_empty() {
        if let Some(parsed) = decode_sse_line(&buffer.clone()) {
            lines.push(parsed);
        }
        buffer.clear();
    }
    lines
}

fn decode_sse_line(raw: &[u8]) -> Option<ParsedLine> {
    let line = String::from_utf8_lossy(raw);
    let line = line.trim_end_matches('\r').trim();
    parse_sse_line(line)
}

/// Extracts the delta content from one SSE line. Returns None for
/// non-data lines and keep-alive comments.
fn parse_sse_line(line: &str) -> Option<ParsedLine> {
    let payload = line.strip_prefix("data:")?;
    let payload = payload.trim_start();
    if payload == "[DONE]" {
        return Some(ParsedLine {
            done: true,
            ..Default::default()
        });
    }
    let chunk: StreamChunk = serde_json::from_str(payload).ok()?;
    let delta = chunk.choices.first().map(|c| &c.delta);
    Some(ParsedLine {
        content: delta.and_then(|d| d.content.clone()).filter(|c| !c.is_empty()),
        reasoning: delta
            .and_then(|d| d.reasoning_content.clone())
            .filter(|c| !c.is_empty()),
        done: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::config::{LlmModelConfig, LlmProviderPreset};

    fn openai_provider(base: &str) -> LlmProviderConfig {
        LlmProviderConfig {
            id: "p1".into(),
            name: "P1".into(),
            preset: LlmProviderPreset::Custom,
            kind: LlmProviderKind::OpenAiCompatible,
            base_url: base.into(),
            api_version: None,
            models: vec![LlmModelConfig {
                id: "m1".into(),
                model_id: "gpt-4o".into(),
                display_name: "GPT-4o".into(),
            }],
            has_api_key: false,
        }
    }

    #[test]
    fn chat_url_openai_compatible_trims_trailing_slash() {
        let provider = openai_provider("https://api.openai.com/v1/");
        assert_eq!(
            chat_url(&provider, "gpt-4o").unwrap(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn chat_url_azure_uses_deployment_path_and_version() {
        let mut provider = openai_provider("https://myres.openai.azure.com");
        provider.kind = LlmProviderKind::AzureOpenAi;
        provider.api_version = Some("2024-10-21".into());
        assert_eq!(
            chat_url(&provider, "my-deployment").unwrap(),
            "https://myres.openai.azure.com/openai/deployments/my-deployment/chat/completions?api-version=2024-10-21"
        );
    }

    #[test]
    fn chat_url_rejects_empty_base_and_model() {
        assert!(chat_url(&openai_provider("   "), "m").is_err());
        assert!(chat_url(&openai_provider("https://x"), "").is_err());
    }

    #[test]
    fn auth_headers_differ_by_kind() {
        let mut provider = openai_provider("https://x");
        provider.kind = LlmProviderKind::OpenAiCompatible;
        assert!(headers_for(&provider, "sk-1")[0].0 == "Authorization");
        provider.kind = LlmProviderKind::AzureOpenAi;
        assert!(headers_for(&provider, "sk-1")[0].0 == "api-key");
    }

    // Feature: ai-assistant, Property 5: SSE parsing reassembles deltas losslessly
    #[test]
    fn sse_parser_reassembles_delta_sequence() {
        let lines = [
            "data: {\"choices\":[{\"delta\":{\"content\":\"你\"}}]}",
            "",
            "data: {\"choices\":[{\"delta\":{\"content\":\"好\"}}]}",
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}", // no content
            ": keep-alive comment",
            "data: {\"choices\":[{\"delta\":{\"content\":\"！\"}}]}",
            "data: [DONE]",
        ];

        let mut assembled = String::new();
        let mut saw_done = false;
        for line in lines {
            if let Some(parsed) = parse_sse_line(line) {
                assembled.push_str(parsed.content.as_deref().unwrap_or(""));
                saw_done |= parsed.done;
            }
        }
        assert_eq!(assembled, "你好！");
        assert!(saw_done);

        // Deltas split across chunk boundaries must reassemble identically:
        // feed the same payload in arbitrary 3-byte chunks through the
        // line-buffering helper.
        let full_payload = "data: {\"choices\":[{\"delta\":{\"content\":\"世界\"}}]}\n\n";
        let mut buffer: Vec<u8> = Vec::new();
        let mut split_assembled = String::new();
        for chunk in full_payload.as_bytes().chunks(3) {
            for line in drain_sse_deltas(&mut buffer, chunk, false) {
                split_assembled.push_str(line.content.as_deref().unwrap_or(""));
            }
        }
        for line in drain_sse_deltas(&mut buffer, &[], true) {
            split_assembled.push_str(line.content.as_deref().unwrap_or(""));
        }
        assert_eq!(split_assembled, "世界");
        assert!(buffer.is_empty());
    }

    #[test]
    fn sse_parser_ignores_malformed_payloads() {
        assert_eq!(parse_sse_line("data: not json"), None);
        assert_eq!(parse_sse_line("event: message"), None);
        assert!(parse_sse_line("data: [DONE]").unwrap().done);
    }

    // Feature: ai-assistant, Property 5 (continued): reasoning deltas are
    // parsed separately from content deltas
    #[test]
    fn sse_parser_extracts_reasoning_content() {
        let parsed = parse_sse_line(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"让我想想\"}}]}",
        )
        .unwrap();
        assert_eq!(parsed.reasoning.as_deref(), Some("让我想想"));
        assert_eq!(parsed.content, None);

        let parsed = parse_sse_line(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"然后\",\"content\":\"答\"}}]}",
        )
        .unwrap();
        assert_eq!(parsed.reasoning.as_deref(), Some("然后"));
        assert_eq!(parsed.content.as_deref(), Some("答"));
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::llm::config::{LlmModelConfig, LlmProviderKind, LlmProviderPreset};

    /// Live integration test against DashScope's OpenAI-compatible endpoint.
    /// Run with: DASHSCOPE_KEY=sk-... cargo test --lib live -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_dashscope_stream_chat() {
        let Ok(key) = std::env::var("DASHSCOPE_KEY") else {
            panic!("set DASHSCOPE_KEY to run this test");
        };
        let provider = LlmProviderConfig {
            id: "live".into(),
            name: "DashScope".into(),
            preset: LlmProviderPreset::Dashscope,
            kind: LlmProviderKind::OpenAiCompatible,
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            api_version: None,
            models: vec![LlmModelConfig {
                id: "m".into(),
                model_id: "qwen-turbo".into(),
                display_name: "Qwen Turbo".into(),
            }],
            has_api_key: false,
        };

        test_connection(&provider, &key)
            .await
            .expect("test_connection should succeed");

        let messages = vec![LlmChatMessage {
            role: "user".into(),
            content: "用一句话介绍你自己".into(),
        }];
        let mut events = Vec::new();
        stream_chat_events(
            &provider,
            &key,
            &messages,
            None,
            &CancellationToken::new(),
            |e| {
                events.push(e);
            },
        )
        .await;

        let deltas: Vec<&str> = events
            .iter()
            .filter_map(|e| e.delta.as_deref())
            .collect();
        let assembled: String = deltas.concat();
        println!("event kinds: {:?}", events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>());
        println!("assembled: {}", assembled);
        assert!(
            events.iter().any(|e| e.kind == "done"),
            "stream must end with done; events: {:?}",
            events.iter().map(|e| (e.kind.as_str(), e.message.clone())).collect::<Vec<_>>()
        );
        assert!(!assembled.is_empty(), "deltas must be non-empty");
    }
}

#[cfg(test)]
mod system_prompt_tests {
    use super::*;

    // Feature: ai-assistant, Property 7: grounding rule is always present
    #[test]
    fn system_prompt_always_requires_cloud_first() {
        let empty = build_system_prompt(&[]);
        assert!(empty.contains("(none)"));
        // The consent rule must appear with and without context files.
        assert!(empty.contains("Ask the user whether to answer from internet"));
        assert!(empty.contains("Never invent file contents"));

        let files = vec![
            LlmContextFile {
                name: "TLOB report 2025.pdf".into(),
                path: "AE&TS/03. TLOB".into(),
                web_url: "https://fuchschina.sharepoint.cn/...".into(),
                account_name: "work".into(),
                excerpt: None,
            },
            LlmContextFile {
                name: "notes.md".into(),
                path: "R&D".into(),
                web_url: "https://fuchschina.sharepoint.cn/n".into(),
                account_name: "work".into(),
                excerpt: Some("配方 B 配比：A 组分 60%".into()),
            },
        ];
        let with_files = build_system_prompt(&files);
        assert!(with_files.contains("TLOB report 2025.pdf"));
        assert!(with_files.contains("AE&TS/03. TLOB"));
        assert!(with_files.contains("[account: work]"));
        assert!(!with_files.contains("excerpt: \"\"")); // name-only file has no excerpt line
        assert!(with_files.contains("excerpt: \"配方 B 配比：A 组分 60%\""));
        assert!(with_files.contains("Ask the user whether to answer from internet"));
    }
}
