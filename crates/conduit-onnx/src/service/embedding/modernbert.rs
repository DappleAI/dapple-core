//! ModernBERT embedding backend using ONNX Runtime.
//!
//! This module provides embedding generation using pre-converted ONNX models.
//! Models should be converted once using the Python conversion scripts and
//! stored at the path specified by `ONNX_MODEL_PATH`.

use super::{EmbedBatch, EmbeddingBackend, EmbeddingConfig};
use crate::error::OnnxServiceError;
use crate::service::onnx::OnnxBackend;
use async_trait::async_trait;
use ndarray::{Array2, Array3, ArrayView3};
use ort::session::Session;
use ort::value::TensorRef;
use tokenizers::Tokenizer;
use tokio::sync::Mutex;

/// Hard ceiling on tokens per sequence.
///
/// This is a MEMORY BACKSTOP, not a tuning knob. The tokenizer applies no
/// truncation of its own, so without this cap the sequence width is chosen by
/// whatever the caller sent, and attention memory grows with its SQUARE.
///
/// That is not hypothetical. On 2026-08-02 the RAG path embedded a whole
/// prompt as its query (`rag.rs` falls back to `prompt_text` when no
/// `query_hint` is set); an EDGAR task's 135,591-char prompt tokenized to
/// ~30,000 tokens, and one such request took conduit-onnx from a 2.6Gi idle
/// baseline to OOMKilled (exit 137) against a 12Gi limit in **3 seconds**.
/// Every guide sync in flight at that moment died with an opaque
/// "error sending request for url (http://conduit-onnx:8083/embed)".
///
/// **4,096, not the model's 8,192 context window** — measured, not derived.
/// A single 8,192-token sequence OOM-killed the pod on its own (verified
/// 2026-08-02 against the 12Gi limit); 4,096 returned in 14.9s and left the
/// pod at its 2.5Gi baseline. It also sits comfortably above the largest real
/// guide block (~2,863 tokens), so ordinary content is never truncated.
///
/// Note `EMBED_BATCH_SIZE` in dapple-prompt-api CANNOT substitute for this:
/// it bounds how MANY texts share a call, never how long one is. The killing
/// request was a batch of ONE.
pub const MAX_SEQUENCE_TOKENS: usize = 4_096;

/// Ceiling on `group_size * max_len^2` for a single ONNX run.
///
/// The per-sequence cap alone is NOT sufficient, and that is the whole reason
/// this constant exists. Peak memory tracks the PRODUCT of batch size and
/// sequence length squared, so bounding either factor while the other floats
/// leaves the product unbounded. Measured on 2026-08-02, 12Gi limit:
///
/// | group × len²        | area   | outcome            |
/// |---------------------|--------|--------------------|
/// | 1 × 4,096²          | 16.8M  | **ok**, 14.9s      |
/// | 1 × 8,192²          | 67.1M  | OOMKilled          |
/// | 8 × 4,096²          | 134M   | OOMKilled          |
///
/// So the budget is set to the one area proven to survive. Sequences shorter
/// than the cap pack proportionally more per group (a 100-token block allows
/// ~1,600 of them), so typical guide content still batches fully and pays no
/// throughput penalty — only the rare long block splits.
///
/// Keep `MAX_SEQUENCE_TOKENS^2 <= MAX_BATCH_TOKEN_AREA` so a single oversized
/// text always still forms a runnable group of one.
pub const MAX_BATCH_TOKEN_AREA: usize = MAX_SEQUENCE_TOKENS * MAX_SEQUENCE_TOKENS;

/// Enforced at COMPILE time: a lone over-cap text must always still form a
/// runnable group of one. If someone lowers the area budget below one
/// full-width sequence, this fails the build rather than deadlocking a group
/// that can never fit.
const _: () = assert!(MAX_SEQUENCE_TOKENS * MAX_SEQUENCE_TOKENS <= MAX_BATCH_TOKEN_AREA);

/// ModernBERT embedding backend using ONNX Runtime
///
/// Uses pre-converted ONNX models for inference. Models are expected to be
/// at `{ONNX_MODEL_PATH}/modernbert-embed-large/` with:
/// - `model.onnx` - The converted ONNX model
/// - `tokenizer.json` - The tokenizer configuration
pub struct ModernBertBackend {
    model_name: String,
    dimension: usize,
    /// Per-sequence token ceiling; see [`MAX_SEQUENCE_TOKENS`].
    max_sequence_tokens: usize,
    /// `group_size * max_len^2` ceiling; see [`MAX_BATCH_TOKEN_AREA`].
    max_batch_area: usize,
    onnx_backend: OnnxBackend,
    /// Lazy-loaded model state (Mutex for mutable session access)
    state: Mutex<Option<ModernBertState>>,
}

struct ModernBertState {
    session: Session,
    tokenizer: Tokenizer,
}

impl ModernBertBackend {
    /// Eagerly load the ONNX model so OOM or missing-model errors surface at startup.
    pub async fn warmup(&self) -> Result<(), OnnxServiceError> {
        let mut state_guard = self.state.lock().await;
        if state_guard.is_none() {
            *state_guard = Some(self.load_model().await?);
        }
        Ok(())
    }

    /// Create a new ModernBERT backend (model is loaded lazily on first use)
    pub async fn new(config: &EmbeddingConfig) -> Result<Self, OnnxServiceError> {
        let onnx_backend = OnnxBackend::new().map_err(|e| {
            OnnxServiceError::EmbeddingError(format!("Failed to initialize ONNX Runtime: {}", e))
        })?;

        // Map HuggingFace model ID to ONNX model directory name
        let model_name = Self::model_id_to_name(&config.model_id);

        let max_sequence_tokens = config.max_sequence_tokens.max(1);

        Ok(Self {
            model_name,
            dimension: config.dimension,
            max_sequence_tokens,
            // Never let the budget fall below one full-width sequence, or a
            // single long text could not form a runnable group.
            max_batch_area: config
                .max_batch_token_area
                .max(max_sequence_tokens.saturating_mul(max_sequence_tokens)),
            onnx_backend,
            state: Mutex::new(None),
        })
    }

    /// Convert a HuggingFace model ID to an ONNX model directory name
    fn model_id_to_name(model_id: &str) -> String {
        // e.g., "lightonai/modernbert-embed-large" -> "modernbert-embed-large"
        model_id
            .split('/')
            .next_back()
            .unwrap_or(model_id)
            .to_string()
    }

    /// Load the model and tokenizer (called lazily on first use)
    async fn load_model(&self) -> Result<ModernBertState, OnnxServiceError> {
        log::info!(
            "Loading ModernBERT ONNX model: {} with {}",
            self.model_name,
            self.onnx_backend.execution_provider()
        );

        // Check if model exists
        if !self.onnx_backend.model_exists(&self.model_name) {
            return Err(OnnxServiceError::EmbeddingError(format!(
                "ONNX model not found at {}. Please run the model conversion script first.",
                self.onnx_backend
                    .model_base_path()
                    .join(&self.model_name)
                    .display()
            )));
        }

        // Load ONNX session
        let session = self
            .onnx_backend
            .load_model(&self.model_name)
            .map_err(|e| {
                OnnxServiceError::EmbeddingError(format!("Failed to load ONNX model: {}", e))
            })?;

        // Load tokenizer
        let tokenizer_path = self.onnx_backend.tokenizer_path(&self.model_name);
        let tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(|e| {
            OnnxServiceError::EmbeddingError(format!(
                "Failed to load tokenizer from {}: {}",
                tokenizer_path.display(),
                e
            ))
        })?;

        log::info!("ModernBERT ONNX model loaded successfully");

        Ok(ModernBertState { session, tokenizer })
    }

    /// Perform mean pooling on token embeddings with attention mask
    fn mean_pool(embeddings: ArrayView3<f32>, attention_mask: &Array2<i64>) -> Array2<f32> {
        let batch_size = embeddings.shape()[0];
        let seq_len = embeddings.shape()[1];
        let hidden_size = embeddings.shape()[2];

        let mut result = Array2::<f32>::zeros((batch_size, hidden_size));

        for batch_idx in 0..batch_size {
            let mut sum = vec![0.0f32; hidden_size];
            let mut count = 0.0f32;

            for seq_idx in 0..seq_len.min(attention_mask.shape()[1]) {
                let mask_val = attention_mask[[batch_idx, seq_idx]] as f32;
                if mask_val > 0.0 {
                    for h in 0..hidden_size {
                        sum[h] += embeddings[[batch_idx, seq_idx, h]] * mask_val;
                    }
                    count += mask_val;
                }
            }

            // Average
            let count = count.max(1e-9);
            for h in 0..hidden_size {
                result[[batch_idx, h]] = sum[h] / count;
            }
        }

        result
    }

    /// L2 normalize embeddings
    fn l2_normalize(embeddings: &Array2<f32>) -> Array2<f32> {
        let mut result = embeddings.clone();

        for batch_idx in 0..embeddings.shape()[0] {
            let row = embeddings.row(batch_idx);
            let norm: f32 = row.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);

            for h in 0..embeddings.shape()[1] {
                result[[batch_idx, h]] /= norm;
            }
        }

        result
    }

    /// Truncate embeddings to target dimension (Matryoshka)
    fn truncate_dimension(embeddings: &Array2<f32>, target_dim: usize) -> Array2<f32> {
        let current_dim = embeddings.shape()[1];

        if target_dim >= current_dim {
            return embeddings.clone();
        }

        embeddings.slice(ndarray::s![.., ..target_dim]).to_owned()
    }

    /// Run ONE already-bounded group of encodings through the session.
    ///
    /// The caller is responsible for having chosen `max_len` and the group
    /// size such that `group.len() * max_len^2` is within budget — this
    /// function does no bounding of its own. Sequences longer than `max_len`
    /// are truncated by the copy loop below (it reads `ids[i]` only for
    /// `i < max_len`).
    fn run_group(
        state: &mut ModernBertState,
        group: &[tokenizers::Encoding],
        max_len: usize,
        dimension: usize,
    ) -> Result<Vec<Vec<f32>>, OnnxServiceError> {
        let batch_size = group.len();

        // Prepare input tensors with padding
        let mut input_ids: Vec<i64> = Vec::with_capacity(batch_size * max_len);
        let mut attention_mask: Vec<i64> = Vec::with_capacity(batch_size * max_len);

        for encoding in group {
            let ids = encoding.get_ids();
            let mask = encoding.get_attention_mask();

            // Pad to max_len (and truncate anything longer)
            for i in 0..max_len {
                if i < ids.len() {
                    input_ids.push(ids[i] as i64);
                    attention_mask.push(mask[i] as i64);
                } else {
                    input_ids.push(0); // PAD token
                    attention_mask.push(0);
                }
            }
        }

        // Create ndarray views for ONNX Runtime
        let input_ids_array =
            Array2::from_shape_vec((batch_size, max_len), input_ids).map_err(|e| {
                OnnxServiceError::EmbeddingError(format!("Failed to create input_ids array: {}", e))
            })?;

        let attention_mask_array =
            Array2::from_shape_vec((batch_size, max_len), attention_mask.clone()).map_err(|e| {
                OnnxServiceError::EmbeddingError(format!(
                    "Failed to create attention_mask array: {}",
                    e
                ))
            })?;

        // Create TensorRef inputs for ONNX Runtime
        let input_ids_tensor = TensorRef::from_array_view(input_ids_array.view()).map_err(|e| {
            OnnxServiceError::EmbeddingError(format!("Failed to create input_ids tensor: {}", e))
        })?;
        let attention_mask_tensor = TensorRef::from_array_view(attention_mask_array.view())
            .map_err(|e| {
                OnnxServiceError::EmbeddingError(format!(
                    "Failed to create attention_mask tensor: {}",
                    e
                ))
            })?;

        // Run ONNX inference (session.run requires &mut self)
        let outputs = state
            .session
            .run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor
            ])
            .map_err(|e| {
                OnnxServiceError::EmbeddingError(format!("ONNX inference failed: {}", e))
            })?;

        // Extract hidden states from output
        // Output is typically "last_hidden_state" with shape [batch_size, seq_len, hidden_size]
        let output_tensor = if outputs.len() == 0 {
            return Err(OnnxServiceError::EmbeddingError(
                "No output tensor from model".to_string(),
            ));
        } else {
            outputs.get("last_hidden_state").unwrap_or(&outputs[0])
        };

        let hidden_states: ndarray::ArrayViewD<'_, f32> =
            output_tensor.try_extract_array::<f32>().map_err(|e| {
                OnnxServiceError::EmbeddingError(format!("Failed to extract output tensor: {}", e))
            })?;

        // Verify output is 3D [batch_size, seq_len, hidden_size]
        let shape = hidden_states.shape();
        if shape.len() != 3 {
            return Err(OnnxServiceError::EmbeddingError(format!(
                "Expected 3D output tensor, got {}D",
                shape.len()
            )));
        }

        // Convert to owned 3D array
        let hidden_3d: Array3<f32> = hidden_states
            .into_dimensionality::<ndarray::Ix3>()
            .map_err(|e| {
                OnnxServiceError::EmbeddingError(format!("Failed to convert to 3D: {}", e))
            })?
            .to_owned();

        // Create attention mask array for pooling
        let attention_mask_array_i64 =
            Array2::from_shape_vec((batch_size, max_len), attention_mask).map_err(|e| {
                OnnxServiceError::EmbeddingError(format!(
                    "Failed to create attention_mask array: {}",
                    e
                ))
            })?;

        // Mean pooling
        let pooled = Self::mean_pool(hidden_3d.view(), &attention_mask_array_i64);

        // Truncate to target dimension (Matryoshka)
        let truncated = Self::truncate_dimension(&pooled, dimension);

        // L2 normalize
        let normalized = Self::l2_normalize(&truncated);

        Ok(normalized.outer_iter().map(|row| row.to_vec()).collect())
    }

    /// Split encodings into consecutive groups whose `count * max_len^2`
    /// stays within `area_budget`, truncating each sequence at `max_tokens`.
    ///
    /// Returns `(start, end, max_len)` per group. Order is preserved, so
    /// `embeddings[i]` still corresponds to `texts[i]` — callers rely on that.
    /// A group always holds at least one item, so an item that alone exceeds
    /// the budget still runs (it cannot: `max_tokens^2 <= area_budget` is
    /// enforced in the constructor).
    fn plan_groups(
        token_lens: &[usize],
        max_tokens: usize,
        area_budget: usize,
    ) -> Vec<(usize, usize, usize)> {
        let mut groups = Vec::new();
        let mut start = 0;

        while start < token_lens.len() {
            let mut group_max = 0usize;
            let mut end = start;

            while end < token_lens.len() {
                let len = token_lens[end].min(max_tokens);
                let candidate_max = group_max.max(len);
                let count = end - start + 1;
                // Padding widens EVERY member to the group's longest, so the
                // cost of adding one long text is paid by the whole group.
                let area = count.saturating_mul(candidate_max.saturating_mul(candidate_max));

                if count > 1 && area > area_budget {
                    break;
                }
                group_max = candidate_max;
                end += 1;
            }

            groups.push((start, end, group_max.max(1)));
            start = end;
        }

        groups
    }
}

#[async_trait]
impl EmbeddingBackend for ModernBertBackend {
    async fn embed_batch(&self, texts: &[String]) -> Result<EmbedBatch, OnnxServiceError> {
        if texts.is_empty() {
            return Ok(EmbedBatch {
                embeddings: Vec::new(),
                truncated_count: 0,
            });
        }

        // Acquire lock and lazily initialize model
        let mut state_guard = self.state.lock().await;
        if state_guard.is_none() {
            *state_guard = Some(self.load_model().await?);
        }
        let state = state_guard.as_mut().unwrap();

        // Tokenize all texts
        let encodings = state
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| OnnxServiceError::EmbeddingError(format!("Tokenization failed: {}", e)))?;

        // Count what will be truncated. The truncation itself happens in
        // `run_group`; this both warns and RETURNS the count, so a caller that
        // persists these vectors (guide ingest) can refuse a lossy index
        // instead of discovering the loss only from this pod's log.
        let longest = encodings
            .iter()
            .map(|e| e.get_ids().len())
            .max()
            .unwrap_or(0);
        let truncated_count = encodings
            .iter()
            .filter(|e| e.get_ids().len() > self.max_sequence_tokens)
            .count();
        if truncated_count > 0 {
            log::warn!(
                "Embedding input exceeds the sequence cap: {}/{} texts truncated \
                 from up to {} tokens to {}. The embedding covers only the \
                 leading {} tokens of those texts.",
                truncated_count,
                encodings.len(),
                longest,
                self.max_sequence_tokens,
                self.max_sequence_tokens,
            );
        }

        // Split into groups whose `count * max_len^2` is within budget, then
        // run them sequentially. This — not the caller's batch size — is what
        // makes the service safe: peak memory is bounded no matter what
        // arrives (see MAX_BATCH_TOKEN_AREA).
        let token_lens: Vec<usize> = encodings.iter().map(|e| e.get_ids().len()).collect();
        let groups = Self::plan_groups(&token_lens, self.max_sequence_tokens, self.max_batch_area);

        if groups.len() > 1 {
            log::debug!(
                "Splitting {} texts into {} memory-bounded groups (longest {} tokens)",
                encodings.len(),
                groups.len(),
                longest.min(self.max_sequence_tokens),
            );
        }

        let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
        for (start, end, max_len) in groups {
            let group = Self::run_group(state, &encodings[start..end], max_len, self.dimension)?;
            embeddings.extend(group);
        }

        Ok(EmbedBatch {
            embeddings,
            truncated_count,
        })
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn name(&self) -> &str {
        "modernbert-onnx"
    }

    async fn warmup(&self) -> Result<(), OnnxServiceError> {
        ModernBertBackend::warmup(self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Total area actually submitted to ONNX for a plan — the quantity that
    /// must stay bounded, since it is what OOM-killed the pod.
    fn worst_group_area(groups: &[(usize, usize, usize)]) -> usize {
        groups
            .iter()
            .map(|(s, e, max_len)| (e - s) * max_len * max_len)
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn plan_groups_keeps_short_texts_in_one_group() {
        // Typical guide content: ~100-token blocks. These must still batch
        // fully, or we would pay a throughput penalty for the common case.
        let lens = vec![100; 8];
        let groups = plan(&lens);
        assert_eq!(groups.len(), 1, "short texts should not be split");
        assert_eq!(groups[0], (0, 8, 100));
    }

    fn plan(lens: &[usize]) -> Vec<(usize, usize, usize)> {
        ModernBertBackend::plan_groups(lens, MAX_SEQUENCE_TOKENS, MAX_BATCH_TOKEN_AREA)
    }

    #[test]
    fn plan_groups_bounds_the_batch_that_oom_killed_the_pod() {
        // The measured failure: 8 texts each over the cap => 8 * 4096^2.
        let lens = vec![30_361; 8];
        let groups = plan(&lens);

        assert!(
            worst_group_area(&groups) <= MAX_BATCH_TOKEN_AREA,
            "planned area {} exceeds budget {}",
            worst_group_area(&groups),
            MAX_BATCH_TOKEN_AREA
        );
        // Each over-cap text must run alone at 4096 wide.
        assert_eq!(groups.len(), 8);
        assert!(groups
            .iter()
            .all(|(_, _, max_len)| *max_len == MAX_SEQUENCE_TOKENS));
    }

    #[test]
    fn plan_groups_covers_every_input_exactly_once_in_order() {
        // Order and completeness are the correctness contract: the caller
        // relies on embeddings[i] still matching texts[i].
        let lens = vec![10, 4_000, 50, 30_000, 7, 2_863, 900, 1];
        let groups = plan(&lens);

        let mut cursor = 0;
        for (start, end, _) in &groups {
            assert_eq!(*start, cursor, "groups must be consecutive");
            assert!(end > start, "groups must be non-empty");
            cursor = *end;
        }
        assert_eq!(cursor, lens.len(), "every input must be covered");
        assert!(worst_group_area(&groups) <= MAX_BATCH_TOKEN_AREA);
    }

    #[test]
    fn plan_groups_never_emits_an_empty_or_unrunnable_group() {
        // A single text far over the cap still has to run — as a group of one
        // truncated to the cap, never dropped and never zero-width.
        let groups = plan(&[1_000_000]);
        assert_eq!(groups, vec![(0, 1, MAX_SEQUENCE_TOKENS)]);

        // Zero-length input must not produce a zero-width tensor.
        let groups = plan(&[0]);
        assert_eq!(groups, vec![(0, 1, 1)]);

        assert!(plan(&[]).is_empty());
    }
}
