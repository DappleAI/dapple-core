//! ModernBERT MLM backend using ONNX Runtime.
//!
//! This module provides masked language modeling using pre-converted ONNX models.
//! Models should be converted once using the Python conversion scripts and
//! stored at the path specified by `ONNX_MODEL_PATH`.

use super::{MaskPrediction, MlmBackend, MlmConfig, MlmResult, TokenPrediction};
use crate::error::ConduitError;
use crate::service::onnx::OnnxBackend;
use async_trait::async_trait;
use ndarray::Array2;
use ort::session::Session;
use ort::value::TensorRef;
use tokenizers::Tokenizer;
use tokio::sync::Mutex;

/// ModernBERT MLM backend using ONNX Runtime
///
/// Uses pre-converted ONNX models for inference. Models are expected to be
/// at `{ONNX_MODEL_PATH}/ModernBERT-base/` with:
/// - `model.onnx` - The converted ONNX model (with MLM head)
/// - `tokenizer.json` - The tokenizer configuration
pub struct ModernBertMlmBackend {
    model_name: String,
    onnx_backend: OnnxBackend,
    /// Lazy-loaded model state (Mutex for mutable session access)
    state: Mutex<Option<ModernBertMlmState>>,
}

struct ModernBertMlmState {
    session: Session,
    tokenizer: Tokenizer,
    mask_token_id: u32,
}

impl ModernBertMlmBackend {
    /// Eagerly load the ONNX model so OOM or missing-model errors surface at startup.
    pub async fn warmup(&self) -> Result<(), ConduitError> {
        let mut state_guard = self.state.lock().await;
        if state_guard.is_none() {
            *state_guard = Some(self.load_model().await?);
        }
        Ok(())
    }

    /// Create a new ModernBERT MLM backend (model is loaded lazily on first use)
    pub async fn new(config: &MlmConfig) -> Result<Self, ConduitError> {
        let onnx_backend = OnnxBackend::new().map_err(|e| {
            ConduitError::MlmError(format!("Failed to initialize ONNX Runtime: {}", e))
        })?;

        // Map HuggingFace model ID to ONNX model directory name
        let model_name = Self::model_id_to_name(&config.model_id);

        Ok(Self {
            model_name,
            onnx_backend,
            state: Mutex::new(None),
        })
    }

    /// Convert a HuggingFace model ID to an ONNX model directory name
    fn model_id_to_name(model_id: &str) -> String {
        // e.g., "answerdotai/ModernBERT-base" -> "ModernBERT-base"
        model_id
            .split('/')
            .next_back()
            .unwrap_or(model_id)
            .to_string()
    }

    /// Load the model and tokenizer (called lazily on first use)
    async fn load_model(&self) -> Result<ModernBertMlmState, ConduitError> {
        log::info!(
            "Loading ModernBERT MLM ONNX model: {} with {}",
            self.model_name,
            self.onnx_backend.execution_provider()
        );

        // Check if model exists
        if !self.onnx_backend.model_exists(&self.model_name) {
            return Err(ConduitError::MlmError(format!(
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
            .map_err(|e| ConduitError::MlmError(format!("Failed to load ONNX model: {}", e)))?;

        // Load tokenizer
        let tokenizer_path = self.onnx_backend.tokenizer_path(&self.model_name);
        let tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(|e| {
            ConduitError::MlmError(format!(
                "Failed to load tokenizer from {}: {}",
                tokenizer_path.display(),
                e
            ))
        })?;

        // Find the mask token ID
        let mask_token_id = tokenizer
            .token_to_id("[MASK]")
            .ok_or_else(|| ConduitError::MlmError("Tokenizer has no [MASK] token".to_string()))?;

        log::info!(
            "ModernBERT MLM ONNX model loaded successfully (mask_token_id={})",
            mask_token_id
        );

        Ok(ModernBertMlmState {
            session,
            tokenizer,
            mask_token_id,
        })
    }

    /// Apply softmax to a 1D array
    fn softmax(logits: &[f32]) -> Vec<f32> {
        let max = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exp: Vec<f32> = logits.iter().map(|&x| (x - max).exp()).collect();
        let sum: f32 = exp.iter().sum();
        exp.iter().map(|&x| x / sum).collect()
    }
}

#[async_trait]
impl MlmBackend for ModernBertMlmBackend {
    async fn fill_masks(&self, text: &str, top_k: usize) -> Result<MlmResult, ConduitError> {
        // Acquire lock and lazily initialize model
        let mut state_guard = self.state.lock().await;
        if state_guard.is_none() {
            *state_guard = Some(self.load_model().await?);
        }
        let state = state_guard.as_mut().unwrap();

        // Tokenize the input
        let encoding = state
            .tokenizer
            .encode(text, true)
            .map_err(|e| ConduitError::MlmError(format!("Tokenization failed: {}", e)))?;

        let input_ids = encoding.get_ids();
        let attention_mask = encoding.get_attention_mask();

        // Find mask positions in token sequence
        let mask_positions: Vec<usize> = input_ids
            .iter()
            .enumerate()
            .filter(|(_, &id)| id == state.mask_token_id)
            .map(|(pos, _)| pos)
            .collect();

        // If no masks, return original text
        if mask_positions.is_empty() {
            return Ok(MlmResult {
                filled_text: text.to_string(),
                predictions: Vec::new(),
            });
        }

        let seq_len = input_ids.len();

        // Create input arrays
        let input_ids_i64: Vec<i64> = input_ids.iter().map(|&x| x as i64).collect();
        let attention_mask_i64: Vec<i64> = attention_mask.iter().map(|&x| x as i64).collect();

        let input_ids_array = Array2::from_shape_vec((1, seq_len), input_ids_i64).map_err(|e| {
            ConduitError::MlmError(format!("Failed to create input_ids array: {}", e))
        })?;

        let attention_mask_array = Array2::from_shape_vec((1, seq_len), attention_mask_i64)
            .map_err(|e| {
                ConduitError::MlmError(format!("Failed to create attention_mask array: {}", e))
            })?;

        // Create TensorRef inputs for ONNX Runtime
        let input_ids_tensor = TensorRef::from_array_view(input_ids_array.view()).map_err(|e| {
            ConduitError::MlmError(format!("Failed to create input_ids tensor: {}", e))
        })?;
        let attention_mask_tensor = TensorRef::from_array_view(attention_mask_array.view())
            .map_err(|e| {
                ConduitError::MlmError(format!("Failed to create attention_mask tensor: {}", e))
            })?;

        // Run ONNX inference
        let outputs = state
            .session
            .run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor
            ])
            .map_err(|e| ConduitError::MlmError(format!("ONNX inference failed: {}", e)))?;

        // Extract logits from output
        // For MLM models, output is typically "logits" with shape [batch_size, seq_len, vocab_size]
        let output_tensor = if outputs.len() == 0 {
            return Err(ConduitError::MlmError(
                "No output tensor from model".to_string(),
            ));
        } else {
            outputs.get("logits").unwrap_or(&outputs[0])
        };

        let logits: ndarray::ArrayViewD<'_, f32> =
            output_tensor.try_extract_array::<f32>().map_err(|e| {
                ConduitError::MlmError(format!("Failed to extract output tensor: {}", e))
            })?;

        // Verify output is 3D [batch_size, seq_len, vocab_size]
        let shape = logits.shape();
        if shape.len() != 3 {
            return Err(ConduitError::MlmError(format!(
                "Expected 3D output tensor, got {}D",
                shape.len()
            )));
        }

        let vocab_size = shape[2];

        // Extract predictions for each mask position
        let mut predictions = Vec::new();
        let mut filled_ids = input_ids.to_vec();

        for &mask_pos in &mask_positions {
            // Get logits for this position [vocab_size]
            let pos_logits: Vec<f32> = (0..vocab_size).map(|v| logits[[0, mask_pos, v]]).collect();

            // Apply softmax to get probabilities
            let probs = Self::softmax(&pos_logits);

            // Get top-k predictions
            let mut indexed_probs: Vec<(usize, f32)> =
                probs.iter().enumerate().map(|(i, &p)| (i, p)).collect();
            indexed_probs
                .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            let candidates: Vec<TokenPrediction> = indexed_probs
                .iter()
                .take(top_k)
                .map(|(token_id, score)| {
                    let token = state
                        .tokenizer
                        .id_to_token(*token_id as u32)
                        .unwrap_or_else(|| format!("[UNK:{}]", token_id));
                    TokenPrediction {
                        token,
                        score: *score,
                    }
                })
                .collect();

            // Find character position in original text
            let char_position = encoding
                .get_offsets()
                .get(mask_pos)
                .map(|(start, _)| *start)
                .unwrap_or(0);

            predictions.push(MaskPrediction {
                position: char_position,
                candidates: candidates.clone(),
            });

            // Update filled_ids with best prediction
            if let Some((best_token_id, _)) = indexed_probs.first() {
                filled_ids[mask_pos] = *best_token_id as u32;
            }
        }

        // Decode filled tokens back to text
        let filled_text = state
            .tokenizer
            .decode(&filled_ids, true)
            .map_err(|e| ConduitError::MlmError(format!("Decoding failed: {}", e)))?;

        Ok(MlmResult {
            filled_text,
            predictions,
        })
    }

    fn mask_token(&self) -> &str {
        "[MASK]"
    }

    fn name(&self) -> &str {
        "modernbert-mlm-onnx"
    }

    async fn warmup(&self) -> Result<(), ConduitError> {
        ModernBertMlmBackend::warmup(self).await
    }
}
