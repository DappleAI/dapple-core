use async_trait::async_trait;
use futures::stream;

use super::{LlmBackend, StreamChunk, TokenStream, UsageMetadata};
use crate::error::ConduitError;
use crate::service::model::ModelConfig;
use crate::service::template::RenderedPrompt;

/// Stub LLM backend for testing
/// Echoes input with model info for development and testing
pub struct StubBackend;

impl StubBackend {
    pub fn new() -> Self {
        Self
    }

    /// Build a stub response for testing
    fn build_stub_response(&self, prompt: &RenderedPrompt, model: &ModelConfig) -> String {
        let mut response = format!(
            "[STUB: {} / {} @ {}]\n\nPrompt: {}\n\nMessages:\n",
            model.lineage_key, model.tier, model.provider_key, prompt.prompt_name
        );

        for (i, msg) in prompt.messages.iter().enumerate() {
            let role = msg.role.as_deref().unwrap_or("none");
            response.push_str(&format!(
                "\n--- Message {} [{}] ---\n{}\n",
                i + 1,
                role,
                msg.content
            ));
        }

        response.push_str("\n[END STUB RESPONSE]\n");
        response
    }
}

impl Default for StubBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmBackend for StubBackend {
    fn supports(&self, model: &ModelConfig) -> bool {
        model.lineage_key == "stub"
    }

    async fn execute_streaming(
        &self,
        prompt: &RenderedPrompt,
        model: &ModelConfig,
    ) -> Result<TokenStream, ConduitError> {
        // Build a response from the rendered messages
        let response = self.build_stub_response(prompt, model);

        let usage = UsageMetadata {
            input_tokens: Some(
                prompt
                    .messages
                    .iter()
                    .map(|m| m.content.len() as u32 / 4)
                    .sum(),
            ),
            output_tokens: Some(response.len() as u32 / 4),
            model_id: Some("stub".to_string()),
            ..Default::default()
        };

        // Stream the response in chunks (simulating token streaming), then emit a
        // trailing Final chunk carrying usage (delivered at stream-end).
        let chunks: Vec<String> = response
            .chars()
            .collect::<Vec<_>>()
            .chunks(20)
            .map(|c| c.iter().collect())
            .collect();

        let stream = stream::iter(
            chunks
                .into_iter()
                .map(|c| Ok(StreamChunk::Token(c)))
                .chain(std::iter::once(Ok(StreamChunk::Final(usage)))),
        );
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_supports() {
        let backend = StubBackend::new();

        let stub_model = ModelConfig {
            lineage_key: "stub".to_string(),
            provider_key: "stub".to_string(),
            tier: "default".to_string(),
            ..Default::default()
        };

        let anthropic_model = ModelConfig {
            lineage_key: "claude-3-5".to_string(),
            provider_key: "anthropic".to_string(),
            tier: "sonnet".to_string(),
            ..Default::default()
        };

        assert!(backend.supports(&stub_model));
        assert!(!backend.supports(&anthropic_model));
    }
}
