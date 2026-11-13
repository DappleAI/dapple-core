//! Anthropic LLM backend.
//!
//! Uses rig 0.39's native Anthropic provider for REAL token-by-token streaming
//! and REAL provider-reported token usage. Usage is delivered at stream-END via a
//! trailing [`StreamChunk::Final`] (rig surfaces it on the provider's final
//! response object once the `message_delta` event arrives) — it is not known up
//! front, which is why the LLM stream contract carries usage as the last chunk
//! rather than alongside the stream handle.

use async_stream::stream;
use async_trait::async_trait;
use futures::StreamExt;
use rig_core::completion::{CompletionModel as _, Message};
use rig_core::providers::anthropic;
use rig_core::providers::anthropic::completion::{
    CLAUDE_HAIKU_4_5, CLAUDE_OPUS_4_8, CLAUDE_SONNET_4_6,
};
use rig_core::streaming::StreamedAssistantContent;

// Anthropic model-id fallbacks for `map_model`. These cover families older than
// the constants rig 0.39 still exposes (`CLAUDE_OPUS_4_8`, `CLAUDE_SONNET_4_6`,
// `CLAUDE_HAIKU_4_5`); they are inlined with their canonical Anthropic ids.
const CLAUDE_4_OPUS: &str = "claude-opus-4-0";
const CLAUDE_4_SONNET: &str = "claude-sonnet-4-0";
const CLAUDE_3_5_SONNET: &str = "claude-3-5-sonnet-latest";
const CLAUDE_3_5_HAIKU: &str = "claude-3-5-haiku-latest";

// Claude 5 Sonnet is NEWER than the constants rig 0.39 exposes, so its canonical
// Anthropic id is inlined here (same inlining pattern as the older fallbacks above).
// Reached only by prompts that explicitly pin `.lineage = claude-5 .tier = sonnet`;
// the env-default stays claude-4-6 / sonnet (see model.rs::default_model_from_env).
const CLAUDE_SONNET_5: &str = "claude-sonnet-5";

// Claude 5 Opus, likewise newer than the rig 0.39 constants, inlined with its
// canonical Anthropic id. Reached only by prompts that explicitly pin
// `.lineage = claude-5 .tier = opus`; without a dedicated match arm such a pin
// would fall through the bare `(_, "opus")` fallback and silently resolve to Opus 4.8.
const CLAUDE_OPUS_5: &str = "claude-opus-5";

use super::{LlmBackend, StreamChunk, TokenStream, UsageMetadata};
use crate::error::ConduitError;
use crate::service::model::ModelConfig;
use crate::service::template::{PromptMessage, RenderedPrompt};

/// Anthropic backend backed by rig's native Anthropic provider.
pub struct AnthropicBackend {
    api_key: String,
}

impl AnthropicBackend {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    fn map_model(&self, lineage_key: &str, tier: &str) -> &'static str {
        // Map lineage+tier to Anthropic model ids.
        // Lineage disambiguates tiers like "sonnet" that exist in multiple families.
        match (lineage_key, tier) {
            // Claude 4.6 Sonnet. Undated alias (current Anthropic naming, no date
            // suffix) — always resolves to the latest 4.6 Sonnet snapshot.
            ("claude-4-6", "sonnet") => CLAUDE_SONNET_4_6,
            // Claude 5 Sonnet (undated alias). Only prompts that explicitly pin
            // `.lineage = claude-5` land here; the bare-`sonnet` fallback still
            // resolves to 3.5, and the env-default stays on 4.6 Sonnet.
            ("claude-5", "sonnet") => CLAUDE_SONNET_5,
            // Claude 5 Opus (undated alias). Only prompts that explicitly pin
            // `.lineage = claude-5 .tier = opus` land here; this arm MUST precede the
            // bare `(_, "opus")` fallback below, or such a pin resolves to Opus 4.8.
            ("claude-5", "opus") => CLAUDE_OPUS_5,
            // Current Opus (4.8). The bare `(_, "opus")` fallback resolves here,
            // mirroring the current-Sonnet precedent above — NOT the legacy 4.0.
            ("claude-4-8", "opus") | (_, "opus") | (_, "opus-4-8") => CLAUDE_OPUS_4_8,
            // Current Haiku (4.5).
            ("claude-4-5", "haiku") | (_, "haiku-4-5") => CLAUDE_HAIKU_4_5,
            // Claude 4 (legacy / explicit) models
            (_, "opus-4") => CLAUDE_4_OPUS,
            (_, "sonnet-4") => CLAUDE_4_SONNET,
            // Claude 3.5 models
            ("claude-3-5", "sonnet") | (_, "sonnet-3-5") => CLAUDE_3_5_SONNET,
            ("claude-3-5", "haiku") | (_, "haiku-3-5") => CLAUDE_3_5_HAIKU,
            // Bare "sonnet"/"haiku" without lineage context defaults to 3.5
            (_, "sonnet") => CLAUDE_3_5_SONNET,
            (_, "haiku") => CLAUDE_3_5_HAIKU,
            // Legacy full model names for backwards compatibility
            (_, "claude-4-opus") | (_, "claude-opus-4-20250514") => CLAUDE_4_OPUS,
            (_, "claude-4-sonnet") | (_, "claude-sonnet-4-20250514") => CLAUDE_4_SONNET,
            (_, "claude-3.5-sonnet")
            | (_, "claude-3-5-sonnet-20241022")
            | (_, "claude-3-sonnet") => CLAUDE_3_5_SONNET,
            (_, "claude-3.5-haiku") | (_, "claude-3-5-haiku-20241022") | (_, "claude-3-haiku") => {
                CLAUDE_3_5_HAIKU
            }
            (_, "claude-3-opus") | (_, "claude-3-opus-20240229") => CLAUDE_4_OPUS,
            // Default to latest sonnet for unknown tiers
            _ => {
                log::warn!(
                    "Unknown Anthropic model lineage='{}' tier='{}', defaulting to Claude 3.5 Sonnet",
                    lineage_key,
                    tier
                );
                CLAUDE_3_5_SONNET
            }
        }
    }

    /// Partition RenderedPrompt messages into (system_preamble, chat_history, final_user_prompt).
    ///
    /// - All "system" role messages are concatenated into a single preamble string
    /// - Non-system messages are converted to rig_core::completion::Message
    /// - The last user message is extracted as the final prompt argument
    /// - If no user message exists, a fallback empty prompt is used
    fn partition_messages(messages: &[PromptMessage]) -> (Option<String>, Vec<Message>, String) {
        let mut system_parts: Vec<&str> = Vec::new();
        let mut chat_messages: Vec<Message> = Vec::new();

        for msg in messages {
            let role = msg.role.as_deref().unwrap_or("user");
            match role {
                "system" => system_parts.push(&msg.content),
                "user" | "human" => chat_messages.push(Message::user(&msg.content)),
                "assistant" => chat_messages.push(Message::assistant(&msg.content)),
                _ => chat_messages.push(Message::user(&msg.content)),
            }
        }

        let preamble = if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        };

        // Extract the last user message as the prompt argument
        let final_prompt = if let Some(last_idx) = chat_messages
            .iter()
            .rposition(|m| matches!(m, Message::User { .. }))
        {
            let last_msg = chat_messages.remove(last_idx);
            // Extract text from the Message::User variant
            match last_msg {
                Message::User { content } => content
                    .iter()
                    .filter_map(|c| match c {
                        rig_core::completion::message::UserContent::Text(t) => Some(t.text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            }
        } else {
            String::new()
        };

        (preamble, chat_messages, final_prompt)
    }
}

#[async_trait]
impl LlmBackend for AnthropicBackend {
    fn supports(&self, model: &ModelConfig) -> bool {
        // Support anthropic provider, or claude lineages
        model.provider_key == "anthropic" || model.lineage_key.starts_with("claude")
    }

    async fn execute_streaming(
        &self,
        prompt: &RenderedPrompt,
        model: &ModelConfig,
    ) -> Result<TokenStream, ConduitError> {
        let model_id = self.map_model(&model.lineage_key, &model.tier);

        // Partition messages into system preamble, chat history, and final user prompt
        let (preamble, chat_history, final_prompt) = Self::partition_messages(&prompt.messages);

        // Guard: never send an empty prompt to Anthropic. rig would build an empty
        // text content block, which the API rejects with 400 "text content blocks
        // must be non-empty". An empty render here means a non-LLM prompt (e.g. an
        // image prompt) was routed to the LLM backend — fail clearly instead.
        if final_prompt.trim().is_empty()
            && chat_history.is_empty()
            && preamble.as_deref().map(str::trim).unwrap_or("").is_empty()
        {
            return Err(ConduitError::LlmError(format!(
                "refusing to send an empty prompt to Anthropic for '{}' — a non-LLM prompt was likely routed to the LLM backend",
                prompt.prompt_name
            )));
        }

        log::info!(
            "Executing Anthropic completion (rig-native streaming): model={}, prompt={}, preamble={}, history_len={}, user_prompt_len={}",
            model_id,
            prompt.prompt_name,
            preamble.as_ref().map_or(0, |p| p.len()),
            chat_history.len(),
            final_prompt.len(),
        );

        // Build rig's native Anthropic client from the API key string (we hold the
        // key directly — do NOT use from_env).
        let client = anthropic::Client::builder()
            .api_key(self.api_key.clone())
            .build()
            .map_err(|e| {
                ConduitError::LlmError(format!("Failed to build Anthropic client: {}", e))
            })?;

        let completion_model = anthropic::completion::CompletionModel::new(client, model_id);

        let max_tokens = model.max_tokens.unwrap_or(4096) as u64;
        let temperature = model.temperature.map(|t| t as f64);

        // Build the streaming completion request: preamble (system), chat history,
        // final user prompt, max_tokens, temperature.
        let mut request_builder = completion_model
            .completion_request(final_prompt)
            .messages(chat_history)
            .max_tokens(max_tokens)
            .temperature_opt(temperature);
        if let Some(preamble) = preamble {
            request_builder = request_builder.preamble(preamble);
        }

        let stream_response = request_builder.stream().await.map_err(|e| {
            ConduitError::LlmError(format!("Anthropic streaming request failed: {}", e))
        })?;

        let model_id_owned = model_id.to_string();
        let prompt_name_owned = prompt.prompt_name.clone();

        // Adapt rig's StreamedAssistantContent stream into our StreamChunk contract:
        // - Text deltas -> StreamChunk::Token
        // - the Final response -> StreamChunk::Final with real provider usage
        // - Reasoning / ToolCall variants are ignored (we don't use them here)
        let out_stream = stream! {
            let mut stream_response = std::pin::pin!(stream_response);
            let mut emitted_final = false;
            let mut emitted_text = false;

            while let Some(item) = stream_response.next().await {
                match item {
                    Ok(StreamedAssistantContent::Text(text)) => {
                        if !text.text.is_empty() {
                            emitted_text = true;
                            yield Ok(StreamChunk::Token(text.text));
                        }
                    }
                    Ok(StreamedAssistantContent::Final(res)) => {
                        // rig's provider final response carries real usage (PartialUsage).
                        let input_tokens = res.usage.input_tokens.map(|t| t as u32);
                        let output_tokens = Some(res.usage.output_tokens as u32);

                        log::info!(
                            "Anthropic real usage: input_tokens={:?}, output_tokens={:?}, model={}",
                            input_tokens,
                            output_tokens,
                            model_id_owned,
                        );

                        emitted_final = true;
                        yield Ok(StreamChunk::Final(UsageMetadata {
                            input_tokens,
                            output_tokens,
                            provider_input_tokens: input_tokens,
                            provider_output_tokens: output_tokens,
                            model_id: Some(model_id_owned.clone()),
                        }));
                    }
                    // Ignore reasoning/tool-call variants — not used by this backend.
                    Ok(_) => {}
                    Err(e) => {
                        yield Err(ConduitError::LlmError(format!(
                            "Anthropic stream error: {}",
                            e
                        )));
                        return;
                    }
                }
            }

            // If the provider stream produced NO assistant text, treat it as a
            // FAILED (retryable) completion rather than a silent empty success.
            // A transient Anthropic error/overload can end the stream cleanly
            // with no text and no error (rig does not always surface it as an
            // Err — observed: an ~11s call returning output_tokens=0 with no
            // text and no error). Surfacing it here makes the task fail
            // (retryable via re-dispatch) and visible, instead of yielding empty
            // output the caller parses as "no candidates" (→ unbound slot → dead
            // Next). Current LLM usage is text generation, so an empty completion
            // is never valid; revisit if tool-only responses are ever expected.
            if !emitted_text {
                yield Err(ConduitError::LlmError(format!(
                    "Anthropic returned an empty completion for '{}' (no text streamed) \
                     — likely a transient provider error or overload; treating as a retryable failure",
                    prompt_name_owned
                )));
                return;
            }

            // Defensive: if the provider stream ended without a Final response,
            // still emit a Final so the consumer has a uniform contract.
            if !emitted_final {
                yield Ok(StreamChunk::Final(UsageMetadata {
                    model_id: Some(model_id_owned.clone()),
                    ..Default::default()
                }));
            }
        };

        Ok(Box::pin(out_stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_mapping() {
        let backend = AnthropicBackend::new("test-key".to_string());

        // Claude 4.6 Sonnet lineage (current undated alias)
        assert_eq!(backend.map_model("claude-4-6", "sonnet"), CLAUDE_SONNET_4_6);

        // Current Opus (4.8) — the bare "opus" tier now resolves to 4.8, NOT 4.0.
        assert_eq!(backend.map_model("", "opus"), CLAUDE_OPUS_4_8);
        assert_eq!(backend.map_model("claude-4-8", "opus"), CLAUDE_OPUS_4_8);

        // Claude 3.5 lineage (explicit)
        assert_eq!(backend.map_model("claude-3-5", "sonnet"), CLAUDE_3_5_SONNET);
        assert_eq!(backend.map_model("claude-3-5", "haiku"), CLAUDE_3_5_HAIKU);

        // Bare tiers without specific lineage default to 3.5
        assert_eq!(backend.map_model("", "sonnet"), CLAUDE_3_5_SONNET);
        assert_eq!(backend.map_model("", "haiku"), CLAUDE_3_5_HAIKU);

        // Legacy full model names (backwards compatibility)
        assert_eq!(backend.map_model("", "claude-4-opus"), CLAUDE_4_OPUS);
        assert_eq!(backend.map_model("", "claude-3-opus"), CLAUDE_4_OPUS);
        assert_eq!(backend.map_model("", "claude-3-sonnet"), CLAUDE_3_5_SONNET);
        assert_eq!(backend.map_model("", "claude-3-haiku"), CLAUDE_3_5_HAIKU);
        assert_eq!(
            backend.map_model("", "claude-3.5-sonnet"),
            CLAUDE_3_5_SONNET
        );
        assert_eq!(backend.map_model("", "unknown"), CLAUDE_3_5_SONNET); // fallback
    }

    #[test]
    fn test_supports() {
        let backend = AnthropicBackend::new("test-key".to_string());

        let anthropic_model = ModelConfig {
            lineage_key: "claude-3-5".to_string(),
            provider_key: "anthropic".to_string(),
            tier: "sonnet".to_string(),
            release: Some("20241022".to_string()),
            ..Default::default()
        };

        let openai_model = ModelConfig {
            lineage_key: "gpt-4".to_string(),
            provider_key: "openai".to_string(),
            tier: "turbo".to_string(),
            release: None,
            ..Default::default()
        };

        assert!(backend.supports(&anthropic_model));
        assert!(!backend.supports(&openai_model));
    }

    #[test]
    fn test_partition_messages() {
        use crate::service::template::PromptMessage;

        let messages = vec![
            PromptMessage {
                role: Some("system".to_string()),
                content: "You are a helpful assistant.".to_string(),
            },
            PromptMessage {
                role: Some("user".to_string()),
                content: "Hello!".to_string(),
            },
        ];

        let (preamble, history, final_prompt) = AnthropicBackend::partition_messages(&messages);

        assert_eq!(preamble, Some("You are a helpful assistant.".to_string()));
        assert!(history.is_empty());
        assert_eq!(final_prompt, "Hello!");
    }

    #[test]
    fn test_partition_messages_with_history() {
        use crate::service::template::PromptMessage;

        let messages = vec![
            PromptMessage {
                role: Some("system".to_string()),
                content: "System prompt.".to_string(),
            },
            PromptMessage {
                role: Some("user".to_string()),
                content: "First question".to_string(),
            },
            PromptMessage {
                role: Some("assistant".to_string()),
                content: "First answer".to_string(),
            },
            PromptMessage {
                role: Some("user".to_string()),
                content: "Follow-up question".to_string(),
            },
        ];

        let (preamble, history, final_prompt) = AnthropicBackend::partition_messages(&messages);

        assert_eq!(preamble, Some("System prompt.".to_string()));
        // History should contain the first user msg and the assistant msg
        assert_eq!(history.len(), 2);
        assert!(matches!(&history[0], Message::User { .. }));
        assert!(matches!(&history[1], Message::Assistant { .. }));
        // Final prompt is the last user message
        assert_eq!(final_prompt, "Follow-up question");
    }

    #[test]
    fn test_partition_messages_no_system() {
        use crate::service::template::PromptMessage;

        let messages = vec![PromptMessage {
            role: Some("user".to_string()),
            content: "Just a question".to_string(),
        }];

        let (preamble, history, final_prompt) = AnthropicBackend::partition_messages(&messages);

        assert!(preamble.is_none());
        assert!(history.is_empty());
        assert_eq!(final_prompt, "Just a question");
    }

    #[test]
    fn test_partition_messages_multiple_system() {
        use crate::service::template::PromptMessage;

        let messages = vec![
            PromptMessage {
                role: Some("system".to_string()),
                content: "You are an expert.".to_string(),
            },
            PromptMessage {
                role: Some("system".to_string()),
                content: "Return only JSON.".to_string(),
            },
            PromptMessage {
                role: Some("user".to_string()),
                content: "Generate topics.".to_string(),
            },
        ];

        let (preamble, history, final_prompt) = AnthropicBackend::partition_messages(&messages);

        assert_eq!(
            preamble,
            Some("You are an expert.\n\nReturn only JSON.".to_string())
        );
        assert!(history.is_empty());
        assert_eq!(final_prompt, "Generate topics.");
    }
}
