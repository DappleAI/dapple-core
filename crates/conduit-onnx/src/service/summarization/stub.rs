use super::{SummarizationBackend, SummarizationInput};
use crate::error::OnnxServiceError;
use async_trait::async_trait;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Stub summarization backend for testing
/// Produces deterministic summaries based on input hash
pub struct StubSummarizationBackend {
    max_input_length: usize,
}

impl StubSummarizationBackend {
    pub fn new(max_input_length: usize) -> Self {
        Self { max_input_length }
    }

    /// Generate a deterministic summary from input using hash
    fn hash_to_summary(&self, input: &SummarizationInput) -> String {
        let text = input.to_text();
        let prefix = input.prompt_prefix().unwrap_or("summarize:");

        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        prefix.hash(&mut hasher); // Include prefix in hash for determinism
        let hash = hasher.finish();

        // Count words for context
        let word_count = text.split_whitespace().count();

        // For dialogue input, also count speakers
        let speaker_info = match input {
            SummarizationInput::Dialogue { turns, .. } => {
                let speakers: std::collections::HashSet<_> =
                    turns.iter().map(|t| &t.speaker).collect();
                format!(" ({} speakers, {} turns)", speakers.len(), turns.len())
            }
            SummarizationInput::Text { .. } => String::new(),
        };

        // Generate deterministic but varied summaries based on hash
        let summary_variant = hash % 5;
        let base_summary = match summary_variant {
            0 => format!(
                "The conversation covered {} words{}",
                word_count, speaker_info
            ),
            1 => format!("Summary of {} word input{}", word_count, speaker_info),
            2 => format!("Key points from {} words{}", word_count, speaker_info),
            3 => format!("Discussion overview ({} words{})", word_count, speaker_info),
            _ => format!("Brief of {} word content{}", word_count, speaker_info),
        };

        format!(
            "[STUB SUMMARY] {} (hash: {:08x})",
            base_summary, hash as u32
        )
    }
}

#[async_trait]
impl SummarizationBackend for StubSummarizationBackend {
    async fn summarize(
        &self,
        inputs: &[SummarizationInput],
    ) -> Result<Vec<String>, OnnxServiceError> {
        log::debug!("Stub backend generating {} summaries", inputs.len());
        Ok(inputs.iter().map(|i| self.hash_to_summary(i)).collect())
    }

    fn name(&self) -> &str {
        "stub"
    }

    fn max_input_length(&self) -> usize {
        self.max_input_length
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::summarization::DialogueTurn;

    #[tokio::test]
    async fn test_stub_deterministic() {
        let backend = StubSummarizationBackend::new(512);

        let inputs = vec![SummarizationInput::text("Hello world, this is a test.")];
        let sum1 = backend.summarize(&inputs).await.unwrap();
        let sum2 = backend.summarize(&inputs).await.unwrap();

        // Same input should produce same summary
        assert_eq!(sum1, sum2);
    }

    #[tokio::test]
    async fn test_stub_different_inputs() {
        let backend = StubSummarizationBackend::new(512);

        let sum1 = backend
            .summarize(&[SummarizationInput::text("Hello")])
            .await
            .unwrap();
        let sum2 = backend
            .summarize(&[SummarizationInput::text("Goodbye")])
            .await
            .unwrap();

        // Different inputs should produce different summaries
        assert_ne!(sum1[0], sum2[0]);
    }

    #[tokio::test]
    async fn test_stub_dialogue_input() {
        let backend = StubSummarizationBackend::new(512);

        let turns = vec![
            DialogueTurn {
                speaker: "Alice".to_string(),
                content: "Hello, how are you?".to_string(),
            },
            DialogueTurn {
                speaker: "Bob".to_string(),
                content: "I'm doing great, thanks!".to_string(),
            },
        ];

        let inputs = vec![SummarizationInput::dialogue(turns)];
        let summaries = backend.summarize(&inputs).await.unwrap();

        assert!(summaries[0].contains("[STUB SUMMARY]"));
        assert!(summaries[0].contains("2 speakers"));
        assert!(summaries[0].contains("2 turns"));
    }

    #[tokio::test]
    async fn test_stub_batch() {
        let backend = StubSummarizationBackend::new(512);

        let inputs = vec![
            SummarizationInput::text("First input"),
            SummarizationInput::text("Second input"),
            SummarizationInput::text("Third input"),
        ];

        let summaries = backend.summarize(&inputs).await.unwrap();
        assert_eq!(summaries.len(), 3);

        // All summaries should be different
        assert_ne!(summaries[0], summaries[1]);
        assert_ne!(summaries[1], summaries[2]);
        assert_ne!(summaries[0], summaries[2]);
    }

    #[tokio::test]
    async fn test_stub_empty_input() {
        let backend = StubSummarizationBackend::new(512);
        let summaries = backend.summarize(&[]).await.unwrap();
        assert!(summaries.is_empty());
    }

    #[tokio::test]
    async fn test_stub_different_prefix_produces_different_hash() {
        let backend = StubSummarizationBackend::new(512);

        let sum1 = backend
            .summarize(&[SummarizationInput::text("Hello")])
            .await
            .unwrap();
        let sum2 = backend
            .summarize(&[SummarizationInput::text_with_prefix("Hello", "compress:")])
            .await
            .unwrap();

        // Same text with different prefix should produce different summaries
        assert_ne!(sum1[0], sum2[0]);
    }
}
