//! ModernBERT embedding backend using ONNX Runtime.
//!
//! This module provides embedding generation using pre-converted ONNX models.
//! Models should be converted once using the Python conversion scripts and
//! stored at the path specified by `ONNX_MODEL_PATH`.

use super::{EmbeddingBackend, EmbeddingConfig};
use crate::error::ConduitError;
use crate::service::onnx::OnnxBackend;
use async_trait::async_trait;
use ndarray::{Array2, Array3, ArrayView3};
use ort::session::Session;
use ort::value::TensorRef;
use tokenizers::Tokenizer;
use tokio::sync::Mutex;

/// ModernBERT embedding backend using ONNX Runtime
///
/// Uses pre-converted ONNX models for inference. Models are expected to be
/// at `{ONNX_MODEL_PATH}/modernbert-embed-large/` with:
/// - `model.onnx` - The converted ONNX model
/// - `tokenizer.json` - The tokenizer configuration
pub struct ModernBertBackend {
    model_name: String,
    dimension: usize,
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
    pub async fn warmup(&self) -> Result<(), ConduitError> {
        let mut state_guard = self.state.lock().await;
        if state_guard.is_none() {
            *state_guard = Some(self.load_model().await?);
        }
        Ok(())
    }

    /// Create a new ModernBERT backend (model is loaded lazily on first use)
    pub async fn new(config: &EmbeddingConfig) -> Result<Self, ConduitError> {
        let onnx_backend = OnnxBackend::new().map_err(|e| {
            ConduitError::EmbeddingError(format!("Failed to initialize ONNX Runtime: {}", e))
        })?;

        // Map HuggingFace model ID to ONNX model directory name
        let model_name = Self::model_id_to_name(&config.model_id);

        Ok(Self {
            model_name,
            dimension: config.dimension,
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
    async fn load_model(&self) -> Result<ModernBertState, ConduitError> {
        log::info!(
            "Loading ModernBERT ONNX model: {} with {}",
            self.model_name,
            self.onnx_backend.execution_provider()
        );

        // Check if model exists
        if !self.onnx_backend.model_exists(&self.model_name) {
            return Err(ConduitError::EmbeddingError(format!(
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
                ConduitError::EmbeddingError(format!("Failed to load ONNX model: {}", e))
            })?;

        // Load tokenizer
        let tokenizer_path = self.onnx_backend.tokenizer_path(&self.model_name);
        let tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(|e| {
            ConduitError::EmbeddingError(format!(
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
}

#[async_trait]
impl EmbeddingBackend for ModernBertBackend {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ConduitError> {
        if texts.is_empty() {
            return Ok(Vec::new());
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
            .map_err(|e| ConduitError::EmbeddingError(format!("Tokenization failed: {}", e)))?;

        // Find max length for padding
        let max_len = encodings
            .iter()
            .map(|e| e.get_ids().len())
            .max()
            .unwrap_or(0);

        let batch_size = texts.len();

        // Prepare input tensors with padding
        let mut input_ids: Vec<i64> = Vec::with_capacity(batch_size * max_len);
        let mut attention_mask: Vec<i64> = Vec::with_capacity(batch_size * max_len);

        for encoding in &encodings {
            let ids = encoding.get_ids();
            let mask = encoding.get_attention_mask();

            // Pad to max_len
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
                ConduitError::EmbeddingError(format!("Failed to create input_ids array: {}", e))
            })?;

        let attention_mask_array =
            Array2::from_shape_vec((batch_size, max_len), attention_mask.clone()).map_err(|e| {
                ConduitError::EmbeddingError(format!(
                    "Failed to create attention_mask array: {}",
                    e
                ))
            })?;

        // Create TensorRef inputs for ONNX Runtime
        let input_ids_tensor = TensorRef::from_array_view(input_ids_array.view()).map_err(|e| {
            ConduitError::EmbeddingError(format!("Failed to create input_ids tensor: {}", e))
        })?;
        let attention_mask_tensor = TensorRef::from_array_view(attention_mask_array.view())
            .map_err(|e| {
                ConduitError::EmbeddingError(format!(
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
            .map_err(|e| ConduitError::EmbeddingError(format!("ONNX inference failed: {}", e)))?;

        // Extract hidden states from output
        // Output is typically "last_hidden_state" with shape [batch_size, seq_len, hidden_size]
        let output_tensor = if outputs.len() == 0 {
            return Err(ConduitError::EmbeddingError(
                "No output tensor from model".to_string(),
            ));
        } else {
            outputs.get("last_hidden_state").unwrap_or(&outputs[0])
        };

        let hidden_states: ndarray::ArrayViewD<'_, f32> =
            output_tensor.try_extract_array::<f32>().map_err(|e| {
                ConduitError::EmbeddingError(format!("Failed to extract output tensor: {}", e))
            })?;

        // Verify output is 3D [batch_size, seq_len, hidden_size]
        let shape = hidden_states.shape();
        if shape.len() != 3 {
            return Err(ConduitError::EmbeddingError(format!(
                "Expected 3D output tensor, got {}D",
                shape.len()
            )));
        }

        // Convert to owned 3D array
        let hidden_3d: Array3<f32> = hidden_states
            .into_dimensionality::<ndarray::Ix3>()
            .map_err(|e| ConduitError::EmbeddingError(format!("Failed to convert to 3D: {}", e)))?
            .to_owned();

        // Create attention mask array for pooling
        let attention_mask_array_i64 =
            Array2::from_shape_vec((batch_size, max_len), attention_mask).map_err(|e| {
                ConduitError::EmbeddingError(format!(
                    "Failed to create attention_mask array: {}",
                    e
                ))
            })?;

        // Mean pooling
        let pooled = Self::mean_pool(hidden_3d.view(), &attention_mask_array_i64);

        // Truncate to target dimension (Matryoshka)
        let truncated = Self::truncate_dimension(&pooled, self.dimension);

        // L2 normalize
        let normalized = Self::l2_normalize(&truncated);

        // Convert to Vec<Vec<f32>>
        let embeddings: Vec<Vec<f32>> = normalized.outer_iter().map(|row| row.to_vec()).collect();

        Ok(embeddings)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn name(&self) -> &str {
        "modernbert-onnx"
    }

    async fn warmup(&self) -> Result<(), ConduitError> {
        ModernBertBackend::warmup(self).await
    }
}
