use super::{MaskPrediction, MlmBackend, MlmResult, TokenPrediction};
use crate::error::ConduitError;
use async_trait::async_trait;

/// Stub MLM backend for testing
/// Produces deterministic predictions based on mask position
pub struct StubMlmBackend;

impl StubMlmBackend {
    pub fn new() -> Self {
        Self
    }

    /// Generate deterministic predictions for a mask position
    fn generate_predictions(&self, position: usize, top_k: usize) -> Vec<TokenPrediction> {
        // Generate deterministic "predictions" based on position
        let base_words = ["word", "thing", "item", "value", "token"];

        (0..top_k)
            .map(|i| TokenPrediction {
                token: format!("{}{}", base_words[i % base_words.len()], position),
                score: 1.0 - (i as f32 * 0.1),
            })
            .collect()
    }
}

impl Default for StubMlmBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MlmBackend for StubMlmBackend {
    async fn fill_masks(&self, text: &str, top_k: usize) -> Result<MlmResult, ConduitError> {
        log::debug!("Stub backend filling masks in text (len={})", text.len());

        let mask_token = self.mask_token();
        let mut filled_text = text.to_string();
        let mut predictions = Vec::new();

        // Find all mask positions
        let mut search_start = 0;
        let mut mask_index = 0;

        while let Some(pos) = filled_text[search_start..].find(mask_token) {
            let absolute_pos = search_start + pos;

            // Generate predictions for this mask
            let candidates = self.generate_predictions(mask_index, top_k);

            predictions.push(MaskPrediction {
                position: absolute_pos,
                candidates: candidates.clone(),
            });

            // Replace mask with best prediction
            let replacement = &candidates[0].token;
            filled_text.replace_range(absolute_pos..absolute_pos + mask_token.len(), replacement);

            // Move search start past the replacement
            search_start = absolute_pos + replacement.len();
            mask_index += 1;
        }

        Ok(MlmResult {
            filled_text,
            predictions,
        })
    }

    fn mask_token(&self) -> &str {
        "[MASK]"
    }

    fn name(&self) -> &str {
        "stub"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stub_single_mask() {
        let backend = StubMlmBackend::new();
        let result = backend.fill_masks("Hello [MASK] world", 3).await.unwrap();

        assert!(!result.filled_text.contains("[MASK]"));
        assert_eq!(result.predictions.len(), 1);
        assert_eq!(result.predictions[0].candidates.len(), 3);
    }

    #[tokio::test]
    async fn test_stub_multiple_masks() {
        let backend = StubMlmBackend::new();
        let result = backend
            .fill_masks("The [MASK] is very [MASK]", 2)
            .await
            .unwrap();

        assert!(!result.filled_text.contains("[MASK]"));
        assert_eq!(result.predictions.len(), 2);
    }

    #[tokio::test]
    async fn test_stub_no_masks() {
        let backend = StubMlmBackend::new();
        let result = backend.fill_masks("Hello world", 3).await.unwrap();

        assert_eq!(result.filled_text, "Hello world");
        assert!(result.predictions.is_empty());
    }

    #[tokio::test]
    async fn test_stub_deterministic() {
        let backend = StubMlmBackend::new();
        let result1 = backend.fill_masks("Hello [MASK]", 3).await.unwrap();
        let result2 = backend.fill_masks("Hello [MASK]", 3).await.unwrap();

        assert_eq!(result1.filled_text, result2.filled_text);
        assert_eq!(
            result1.predictions[0].candidates[0].token,
            result2.predictions[0].candidates[0].token
        );
    }

    #[tokio::test]
    async fn test_stub_scores_decreasing() {
        let backend = StubMlmBackend::new();
        let result = backend.fill_masks("[MASK]", 5).await.unwrap();

        let scores: Vec<f32> = result.predictions[0]
            .candidates
            .iter()
            .map(|c| c.score)
            .collect();

        // Scores should be decreasing
        for i in 1..scores.len() {
            assert!(scores[i] < scores[i - 1]);
        }
    }
}
