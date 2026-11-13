//! T5-based summarization backend using ONNX Runtime.
//!
//! This module provides text summarization using pre-converted ONNX models.
//! T5 models are encoder-decoder models and require separate encoder and decoder
//! ONNX files for optimal inference.
//!
//! Expected model structure at `{ONNX_MODEL_PATH}/flan-t5-base-samsum/`:
//! - `encoder_model.onnx` - The encoder model
//! - `decoder_model.onnx` - The decoder model (or `decoder_model_merged.onnx`)
//! - `tokenizer.json` - The tokenizer configuration

use super::{SummarizationBackend, SummarizationConfig, SummarizationInput};
use crate::error::ConduitError;
use crate::service::onnx::OnnxBackend;
use async_trait::async_trait;
use ndarray::{Array2, Array3};
use ort::session::Session;
use ort::value::TensorRef;
use std::sync::Arc;
use tokenizers::Tokenizer;
use tokio::sync::Mutex;

/// Default prompt prefix for summarization
const DEFAULT_PROMPT_PREFIX: &str = "summarize:";

/// T5-based summarization backend using ONNX Runtime
///
/// Uses pre-converted ONNX models for inference. Requires separate encoder
/// and decoder models or a merged model for efficient inference.
pub struct T5SummarizationBackend {
    model_name: String,
    max_input_length: usize,
    max_output_length: usize,
    min_output_length: usize,
    onnx_backend: OnnxBackend,
    /// Lazy-loaded model state (needs Mutex for decode which mutates state)
    state: Arc<Mutex<Option<T5State>>>,
}

struct T5State {
    encoder_session: Session,
    decoder_session: Session,
    tokenizer: Tokenizer,
    eos_token_id: u32,
    pad_token_id: u32,
}

impl T5SummarizationBackend {
    /// Eagerly load the ONNX model so OOM or missing-model errors surface at startup.
    pub async fn warmup(&self) -> Result<(), ConduitError> {
        let mut state_guard = self.state.lock().await;
        if state_guard.is_none() {
            *state_guard = Some(self.load_model().await?);
        }
        Ok(())
    }

    /// Create a new T5 backend (model is loaded lazily on first use)
    pub async fn new(config: &SummarizationConfig) -> Result<Self, ConduitError> {
        let onnx_backend = OnnxBackend::new().map_err(|e| {
            ConduitError::SummarizationError(format!("Failed to initialize ONNX Runtime: {}", e))
        })?;

        // Map HuggingFace model ID to ONNX model directory name
        let model_name = Self::model_id_to_name(&config.model_id);

        Ok(Self {
            model_name,
            max_input_length: config.max_input_length,
            max_output_length: config.max_output_length,
            min_output_length: config.min_output_length,
            onnx_backend,
            state: Arc::new(Mutex::new(None)),
        })
    }

    /// Convert a HuggingFace model ID to an ONNX model directory name
    fn model_id_to_name(model_id: &str) -> String {
        // e.g., "philschmid/flan-t5-base-samsum" -> "flan-t5-base-samsum"
        model_id
            .split('/')
            .next_back()
            .unwrap_or(model_id)
            .to_string()
    }

    /// Load the model and tokenizer (called lazily on first use)
    async fn load_model(&self) -> Result<T5State, ConduitError> {
        log::info!(
            "Loading T5 ONNX models: {} with {}",
            self.model_name,
            self.onnx_backend.execution_provider()
        );

        let model_dir = self.onnx_backend.model_base_path().join(&self.model_name);

        // Check for encoder and decoder models
        let encoder_path = model_dir.join("encoder_model.onnx");
        let decoder_path = model_dir.join("decoder_model.onnx");
        let decoder_merged_path = model_dir.join("decoder_model_merged.onnx");

        if !encoder_path.exists() {
            return Err(ConduitError::SummarizationError(format!(
                "Encoder model not found at {}. Please run the model conversion script first.",
                encoder_path.display()
            )));
        }

        // Load encoder session
        let encoder_session = self
            .onnx_backend
            .load_model_from_path(&encoder_path)
            .map_err(|e| {
                ConduitError::SummarizationError(format!("Failed to load encoder model: {}", e))
            })?;

        // Try merged decoder first, then regular decoder
        let decoder_session = if decoder_merged_path.exists() {
            self.onnx_backend
                .load_model_from_path(&decoder_merged_path)
                .map_err(|e| {
                    ConduitError::SummarizationError(format!(
                        "Failed to load merged decoder model: {}",
                        e
                    ))
                })?
        } else if decoder_path.exists() {
            self.onnx_backend
                .load_model_from_path(&decoder_path)
                .map_err(|e| {
                    ConduitError::SummarizationError(format!("Failed to load decoder model: {}", e))
                })?
        } else {
            return Err(ConduitError::SummarizationError(format!(
                "Decoder model not found at {} or {}. Please run the model conversion script first.",
                decoder_path.display(),
                decoder_merged_path.display()
            )));
        };

        // Load tokenizer
        let tokenizer_path = self.onnx_backend.tokenizer_path(&self.model_name);
        let tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(|e| {
            ConduitError::SummarizationError(format!(
                "Failed to load tokenizer from {}: {}",
                tokenizer_path.display(),
                e
            ))
        })?;

        // Get EOS and PAD token IDs
        let eos_token_id = tokenizer.token_to_id("</s>").unwrap_or(1);
        let pad_token_id = tokenizer.token_to_id("<pad>").unwrap_or(0);

        log::info!(
            "T5 ONNX models loaded successfully (eos={}, pad={})",
            eos_token_id,
            pad_token_id
        );

        Ok(T5State {
            encoder_session,
            decoder_session,
            tokenizer,
            eos_token_id,
            pad_token_id,
        })
    }

    /// Prepare input with prompt prefix (custom or default)
    fn prepare_input(&self, input: &SummarizationInput) -> String {
        let text = input.to_text();
        let prefix = input.prompt_prefix().unwrap_or(DEFAULT_PROMPT_PREFIX);

        // Ensure prefix ends with space for proper tokenization
        if prefix.ends_with(' ') {
            format!("{}{}", prefix, text)
        } else {
            format!("{} {}", prefix, text)
        }
    }

    /// Generate summary using greedy decoding
    fn generate_summary(
        &self,
        state: &mut T5State,
        input_ids: &Array2<i64>,
        attention_mask: &Array2<i64>,
    ) -> Result<String, ConduitError> {
        // Create TensorRef inputs for encoder
        let input_ids_tensor = TensorRef::from_array_view(input_ids.view()).map_err(|e| {
            ConduitError::SummarizationError(format!("Failed to create input_ids tensor: {}", e))
        })?;
        let attention_mask_tensor =
            TensorRef::from_array_view(attention_mask.view()).map_err(|e| {
                ConduitError::SummarizationError(format!(
                    "Failed to create attention_mask tensor: {}",
                    e
                ))
            })?;

        // Run encoder
        let encoder_outputs = state
            .encoder_session
            .run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor
            ])
            .map_err(|e| {
                ConduitError::SummarizationError(format!("Encoder inference failed: {}", e))
            })?;

        // Extract encoder hidden states
        let encoder_hidden_states = if encoder_outputs.len() == 0 {
            return Err(ConduitError::SummarizationError(
                "No encoder output tensor".to_string(),
            ));
        } else {
            encoder_outputs
                .get("last_hidden_state")
                .unwrap_or(&encoder_outputs[0])
        };

        let encoder_output: ndarray::ArrayViewD<'_, f32> = encoder_hidden_states
            .try_extract_array::<f32>()
            .map_err(|e| {
                ConduitError::SummarizationError(format!(
                    "Failed to extract encoder output tensor: {}",
                    e
                ))
            })?;

        // Convert to owned 3D array
        let encoder_output_owned: Array3<f32> = encoder_output
            .into_dimensionality::<ndarray::Ix3>()
            .map_err(|e| {
                ConduitError::SummarizationError(format!(
                    "Failed to convert encoder output to 3D: {}",
                    e
                ))
            })?
            .to_owned();

        // Start with decoder start token (usually pad token for T5)
        let mut generated_tokens: Vec<i64> = vec![state.pad_token_id as i64];

        // Generate tokens autoregressively
        for _ in 0..self.max_output_length {
            let decoder_input =
                Array2::from_shape_vec((1, generated_tokens.len()), generated_tokens.clone())
                    .map_err(|e| {
                        ConduitError::SummarizationError(format!(
                            "Failed to create decoder input: {}",
                            e
                        ))
                    })?;

            // Create TensorRef inputs for decoder
            let decoder_input_tensor =
                TensorRef::from_array_view(decoder_input.view()).map_err(|e| {
                    ConduitError::SummarizationError(format!(
                        "Failed to create decoder_input tensor: {}",
                        e
                    ))
                })?;
            let encoder_output_tensor = TensorRef::from_array_view(encoder_output_owned.view())
                .map_err(|e| {
                    ConduitError::SummarizationError(format!(
                        "Failed to create encoder_output tensor: {}",
                        e
                    ))
                })?;
            let encoder_mask_tensor =
                TensorRef::from_array_view(attention_mask.view()).map_err(|e| {
                    ConduitError::SummarizationError(format!(
                        "Failed to create encoder_mask tensor: {}",
                        e
                    ))
                })?;

            // Run decoder
            let decoder_outputs = state
                .decoder_session
                .run(ort::inputs![
                    "input_ids" => decoder_input_tensor,
                    "encoder_hidden_states" => encoder_output_tensor,
                    "encoder_attention_mask" => encoder_mask_tensor
                ])
                .map_err(|e| {
                    ConduitError::SummarizationError(format!("Decoder inference failed: {}", e))
                })?;

            // Extract logits
            let logits_tensor = if decoder_outputs.len() == 0 {
                return Err(ConduitError::SummarizationError(
                    "No decoder output tensor".to_string(),
                ));
            } else {
                decoder_outputs.get("logits").unwrap_or(&decoder_outputs[0])
            };

            let logits: ndarray::ArrayViewD<'_, f32> =
                logits_tensor.try_extract_array::<f32>().map_err(|e| {
                    ConduitError::SummarizationError(format!(
                        "Failed to extract decoder logits: {}",
                        e
                    ))
                })?;

            // Get logits for last position and find argmax
            let shape = logits.shape();
            let seq_len = shape[1];
            let vocab_size = shape[2];

            // Get logits for last token position
            let last_logits: Vec<f32> = (0..vocab_size)
                .map(|v| logits[[0, seq_len - 1, v]])
                .collect();

            // Greedy selection - take argmax
            let next_token = last_logits
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(idx, _)| idx as i64)
                .unwrap_or(state.eos_token_id as i64);

            // Check for EOS
            if next_token == state.eos_token_id as i64
                && generated_tokens.len() >= self.min_output_length
            {
                break;
            }

            generated_tokens.push(next_token);
        }

        // Decode tokens to text (skip the initial pad token)
        let output_tokens: Vec<u32> = generated_tokens
            .into_iter()
            .skip(1) // Skip decoder start token
            .filter(|&t| t != state.eos_token_id as i64 && t != state.pad_token_id as i64)
            .map(|t| t as u32)
            .collect();

        let summary = state.tokenizer.decode(&output_tokens, true).map_err(|e| {
            ConduitError::SummarizationError(format!("Token decoding failed: {}", e))
        })?;

        Ok(summary.trim().to_string())
    }
}

#[async_trait]
impl SummarizationBackend for T5SummarizationBackend {
    async fn summarize(&self, inputs: &[SummarizationInput]) -> Result<Vec<String>, ConduitError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        // Acquire lock and lazily initialize model
        let mut state_guard = self.state.lock().await;
        if state_guard.is_none() {
            *state_guard = Some(self.load_model().await?);
        }
        let state = state_guard.as_mut().unwrap();

        let mut summaries = Vec::with_capacity(inputs.len());

        // Process each input individually
        for input in inputs {
            let prepared_input = self.prepare_input(input);

            // Tokenize
            let encoding = state
                .tokenizer
                .encode(prepared_input.as_str(), true)
                .map_err(|e| {
                    ConduitError::SummarizationError(format!("Tokenization failed: {}", e))
                })?;

            let mut input_ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
            let mut attention_mask: Vec<i64> = encoding
                .get_attention_mask()
                .iter()
                .map(|&x| x as i64)
                .collect();

            // Truncate if necessary
            if input_ids.len() > self.max_input_length {
                log::warn!(
                    "Input truncated from {} to {} tokens",
                    input_ids.len(),
                    self.max_input_length
                );
                input_ids.truncate(self.max_input_length);
                attention_mask.truncate(self.max_input_length);
            }

            let seq_len = input_ids.len();

            // Create input arrays
            let input_ids_array = Array2::from_shape_vec((1, seq_len), input_ids).map_err(|e| {
                ConduitError::SummarizationError(format!("Failed to create input_ids array: {}", e))
            })?;

            let attention_mask_array = Array2::from_shape_vec((1, seq_len), attention_mask)
                .map_err(|e| {
                    ConduitError::SummarizationError(format!(
                        "Failed to create attention_mask array: {}",
                        e
                    ))
                })?;

            // Generate summary
            let summary = self.generate_summary(state, &input_ids_array, &attention_mask_array)?;
            summaries.push(summary);
        }

        Ok(summaries)
    }

    fn name(&self) -> &str {
        "t5-onnx"
    }

    fn max_input_length(&self) -> usize {
        self.max_input_length
    }

    async fn warmup(&self) -> Result<(), ConduitError> {
        T5SummarizationBackend::warmup(self).await
    }
}
