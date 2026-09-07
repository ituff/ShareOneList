// Memory extraction: distills durable user facts from a recent conversation
// window using the configured default model, merging the result into the
// memory store. Best-effort by design — any failure is swallowed.

use serde::Deserialize;
use serde_json::json;

use crate::llm::client::complete_once;
use crate::llm::client::LlmChatMessage;
use crate::llm::config::LlmProviderConfig;
use crate::store::memory::MemoryStore;

/// Injection budget (requirements 2.2).
pub const MEMORY_MAX_ITEMS: usize = 20;
pub const MEMORY_MAX_CHARS: usize = 2000;

pub const EXTRACTION_SYSTEM_PROMPT: &str = r#"You maintain long-term memory for a desktop AI file-manager assistant. Given the existing memories and a recent conversation window, output the updated memory list.

Rules:
- Keep only durable facts about the user: work context, preferences, recurring file locations, explicit standing instructions. Drop chit-chat and one-off questions.
- Merge duplicates, keeping the richer phrasing. On conflict, prefer the newer conversation.
- At most 20 items. Each item is one short sentence, in the same language as the conversation.
- Never store credentials, passwords, or API keys.

Respond with strict JSON only, no markdown fences, no commentary: {"memories": ["..."]}"#;

/// Parses the model output into a memory list. Tolerates markdown fences and
/// surrounding chatter; returns None when no valid JSON object with a
/// "memories" string array can be located (Property 4).
pub fn parse_extraction(raw: &str) -> Option<Vec<String>> {
    let candidate = extract_json_object(raw)?;
    #[derive(Deserialize)]
    struct ExtractionPayload {
        #[serde(default)]
        memories: Vec<String>,
    }
    let payload: ExtractionPayload = serde_json::from_str(&candidate).ok()?;
    let memories: Vec<String> = payload
        .memories
        .into_iter()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .collect();
    if memories.is_empty() {
        None
    } else {
        Some(memories)
    }
}

/// Finds the first balanced JSON object in `raw` (brace-depth scan, string
/// aware) so fences like ```json … ``` or chatty prefixes are tolerated.
fn extract_json_object(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let start = raw.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, &b) in bytes[start..].iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(raw[start..=start + offset].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Builds the user-turn payload for extraction: existing memories plus the
/// recent conversation window.
pub fn extraction_user_payload(
    existing: &[String],
    conversation: &[LlmChatMessage],
) -> String {
    json!({
        "existingMemories": existing,
        "conversation": conversation
            .iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect::<Vec<_>>(),
    })
    .to_string()
}

/// Selects memories for prompt injection under the budget: at most
/// `max_items` entries and `max_chars` total characters, in the given
/// (pinned-first, most-recent) order. Property 1.
pub fn select_memories_for_prompt(
    contents: Vec<String>,
    max_items: usize,
    max_chars: usize,
) -> Vec<String> {
    let mut selected = Vec::new();
    let mut used = 0usize;
    for content in contents {
        if selected.len() >= max_items {
            break;
        }
        let cost = content.chars().count() + 3; // "- " prefix + newline
        // The first entry alone is truncated to fit so the budget is strict
        // even when a single memory exceeds it.
        if selected.is_empty() && cost > max_chars {
            let budget = max_chars.saturating_sub(3).max(1);
            selected.push(content.chars().take(budget).collect());
            used = max_chars;
            continue;
        }
        if used + cost > max_chars {
            break;
        }
        used += cost;
        selected.push(content);
    }
    selected
}

/// Runs one extraction round: non-streaming completion against the default
/// model, parse, and merge into the store (manual/pinned entries survive —
/// Property 5). Returns the number of memories now stored, or an error
/// string; callers treat errors as "skip this round".
pub async fn extract_and_merge(
    store: &MemoryStore,
    provider: &LlmProviderConfig,
    api_key: &str,
    conversation_id: &str,
) -> Result<usize, String> {
    let existing: Vec<String> = store
        .enabled_for_injection(20)
        .unwrap_or_default()
        .into_iter()
        .map(|m| m.content)
        .collect();

    let messages = vec![
        LlmChatMessage {
            role: "system".into(),
            content: EXTRACTION_SYSTEM_PROMPT.into(),
        },
        LlmChatMessage {
            role: "user".into(),
            content: extraction_user_payload(
                &existing,
                // The caller trims the window; rebuild from the store.
                &store_recent_messages(store, conversation_id),
            ),
        },
    ];
    let raw = complete_once(provider, api_key, &messages, 800)
        .await
        .map_err(|e| e.to_string())?;
    let memories = parse_extraction(&raw).ok_or_else(|| "extraction output was not valid JSON".to_string())?;
    store
        .replace_auto_memories(&memories, conversation_id)
        .map_err(|e| e.to_string())
}

/// Reads the last 10 messages of a conversation for the extraction window.
fn store_recent_messages(store: &MemoryStore, conversation_id: &str) -> Vec<LlmChatMessage> {
    crate::store::chat_history::recent_messages_for_extraction(
        store.chat_history_path(),
        conversation_id,
        10,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Feature: ai-memory, Property 4: tolerant JSON parsing
    #[test]
    fn parser_accepts_fences_and_noise() {
        assert_eq!(
            parse_extraction(r#"{"memories": ["A", "B"]}"#).unwrap(),
            vec!["A", "B"]
        );
        assert_eq!(
            parse_extraction("```json\n{\"memories\": [\"你好\"]}\n```").unwrap(),
            vec!["你好"]
        );
        assert_eq!(
            parse_extraction("Here is the result:\n{\"memories\": [\"X\"]}\nDone.").unwrap(),
            vec!["X"]
        );
        // Nested braces inside string values must not break the scan.
        assert_eq!(
            parse_extraction(r#"{"memories": ["uses {braces} often"]}"#).unwrap(),
            vec!["uses {braces} often"]
        );
    }

    #[test]
    fn parser_rejects_invalid_and_empty() {
        assert!(parse_extraction("not json at all").is_none());
        assert!(parse_extraction("{\"memories\": []}").is_none(), "empty list is a no-op signal");
        assert!(parse_extraction("{\"other\": 1}").is_none());
        assert!(parse_extraction("").is_none());
    }

    // Feature: ai-memory, Property 1: injection budget invariant
    #[test]
    fn selection_respects_item_and_char_budgets() {
        // Deterministic pseudo-random lengths.
        let mut state: u64 = 42;
        let mut contents = Vec::new();
        for _ in 0..50 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let len = 5 + (state >> 33) as usize % 200;
            contents.push("test-char".repeat(len));
        }
        for max_items in [1usize, 3, 20] {
            for max_chars in [50usize, 500, 2000] {
                let selected =
                    select_memories_for_prompt(contents.clone(), max_items, max_chars);
                assert!(selected.len() <= max_items);
                let total: usize = selected.iter().map(|s| s.chars().count() + 3).sum();
                assert!(total <= max_chars, "chars {total} > {max_chars}");
            }
        }
        // A single oversized memory still makes it in alone.
        let selected = select_memories_for_prompt(vec!["x".repeat(3000)], 20, 2000);
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn user_payload_includes_existing_and_conversation() {
        let payload = extraction_user_payload(
            &["已知记忆".into()],
            &[LlmChatMessage {
                role: "user".into(),
                content: "问题".into(),
            }],
        );
        assert!(payload.contains("已知记忆"));
        assert!(payload.contains("问题"));
        assert!(payload.contains("existingMemories"));
    }
}
