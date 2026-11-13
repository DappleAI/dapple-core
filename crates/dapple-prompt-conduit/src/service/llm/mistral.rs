//! Local Mistral inference backend using Candle ML
//!
//! This module provides local LLM inference using Mistral models via Candle,
//! enabling fully offline prompt execution without external API dependencies.

use async_trait::async_trait;
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::mistral::{Config as MistralModelConfig, Model as MistralModel};
use futures::{stream, StreamExt};
use rand::distributions::{Distribution, WeightedIndex};
use rand::SeedableRng;
use std::path::PathBuf;
use std::sync::Arc;
use tokenizers::Tokenizer;
use tokio::sync::{Mutex, OnceCell, RwLock};

use super::{LlmBackend, StreamChunk, TokenStream, UsageMetadata};
use crate::error::ConduitError;
use crate::service::model::ModelConfig;
use crate::service::template::RenderedPrompt;

/// Configuration for Mistral model
#[derive(Debug, Clone)]
pub struct MistralConfig {
    pub model_id: String,
    pub revision: Option<String>,
    pub device: DeviceConfig,
    pub dtype: DTypeConfig,
    pub max_context_length: usize,
    pub hf_token: Option<String>,
}

#[derive(Debug, Clone)]
pub enum DeviceConfig {
    Cpu,
    Cuda(usize),
    Metal,
}

#[derive(Debug, Clone)]
pub enum DTypeConfig {
    F32,
    F16,
    BF16,
}

impl Default for MistralConfig {
    fn default() -> Self {
        Self {
            model_id: "mistralai/Mistral-7B-Instruct-v0.3".to_string(),
            revision: None,
            device: DeviceConfig::Cpu,
            dtype: DTypeConfig::F16,
            max_context_length: 8192, // Conservative default
            hf_token: None,
        }
    }
}

impl MistralConfig {
    pub fn from_env() -> Self {
        Self {
            model_id: std::env::var("MISTRAL_MODEL_ID")
                .unwrap_or_else(|_| "mistralai/Mistral-7B-Instruct-v0.3".to_string()),
            revision: std::env::var("MISTRAL_REVISION").ok(),
            device: DeviceConfig::from_env(),
            dtype: DTypeConfig::from_env(),
            max_context_length: std::env::var("MISTRAL_MAX_CONTEXT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8192),
            hf_token: std::env::var("HF_TOKEN")
                .ok()
                .or_else(|| std::env::var("HUGGINGFACE_TOKEN").ok()),
        }
    }
}

impl DeviceConfig {
    pub fn from_env() -> Self {
        match std::env::var("MISTRAL_DEVICE").as_deref() {
            Ok("cuda") | Ok("cuda:0") => Self::Cuda(0),
            Ok(s) if s.starts_with("cuda:") => {
                let idx = s[5..].parse().unwrap_or(0);
                Self::Cuda(idx)
            }
            Ok("metal") => Self::Metal,
            _ => {
                // Auto-detect based on compile features
                if cfg!(feature = "cuda") {
                    Self::Cuda(0)
                } else if cfg!(feature = "metal") {
                    Self::Metal
                } else {
                    Self::Cpu
                }
            }
        }
    }

    pub fn to_device(&self) -> Result<Device, ConduitError> {
        match self {
            Self::Cpu => Ok(Device::Cpu),
            Self::Cuda(ordinal) => {
                #[cfg(feature = "cuda")]
                {
                    Device::new_cuda(*ordinal)
                        .map_err(|e| ConduitError::LlmError(format!("CUDA init failed: {}", e)))
                }
                #[cfg(not(feature = "cuda"))]
                {
                    let _ = ordinal;
                    Err(ConduitError::LlmError(
                        "CUDA feature not enabled at compile time".to_string(),
                    ))
                }
            }
            Self::Metal => {
                #[cfg(feature = "metal")]
                {
                    Device::new_metal(0)
                        .map_err(|e| ConduitError::LlmError(format!("Metal init failed: {}", e)))
                }
                #[cfg(not(feature = "metal"))]
                {
                    Err(ConduitError::LlmError(
                        "Metal feature not enabled at compile time".to_string(),
                    ))
                }
            }
        }
    }
}

impl DTypeConfig {
    pub fn from_env() -> Self {
        match std::env::var("MISTRAL_DTYPE").as_deref() {
            Ok("f32") => Self::F32,
            Ok("f16") => Self::F16,
            Ok("bf16") => Self::BF16,
            _ => Self::F16, // Good balance of speed/precision
        }
    }

    pub fn to_dtype(&self) -> DType {
        match self {
            Self::F32 => DType::F32,
            Self::F16 => DType::F16,
            Self::BF16 => DType::BF16,
        }
    }
}

/// Sampling parameters for text generation
#[derive(Debug, Clone)]
pub struct SamplingParams {
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: usize,
    pub max_tokens: usize,
    pub stop_sequences: Vec<String>,
    pub seed: Option<u64>,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            max_tokens: 2048,
            stop_sequences: vec!["</s>".to_string(), "[/INST]".to_string()],
            seed: None,
        }
    }
}

impl SamplingParams {
    /// Create from ModelConfig, falling back to defaults for unspecified values
    pub fn from_model_config(model: &ModelConfig) -> Self {
        let defaults = Self::default();
        Self {
            temperature: model.temperature.unwrap_or(defaults.temperature),
            top_p: model.top_p.unwrap_or(defaults.top_p),
            top_k: model.top_k.unwrap_or(defaults.top_k),
            max_tokens: model.max_tokens.unwrap_or(defaults.max_tokens),
            stop_sequences: defaults.stop_sequences,
            seed: None,
        }
    }
}

/// Loaded model state with interior mutability for the model
/// The model needs mutable access for forward pass (KV cache updates)
struct MistralState {
    model: RwLock<MistralModel>,
    tokenizer: Tokenizer,
    device: Device,
    eos_token_id: u32,
}

/// Generator that produces tokens one at a time
pub struct MistralGenerator {
    config: MistralConfig,
    state: OnceCell<MistralState>,
}

impl MistralGenerator {
    /// Create a new generator with lazy loading
    pub fn new(config: MistralConfig) -> Self {
        Self {
            config,
            state: OnceCell::new(),
        }
    }

    /// Get or initialize the model state
    async fn get_state(&self) -> Result<&MistralState, ConduitError> {
        self.state.get_or_try_init(|| self.load_model()).await
    }

    /// Load the model and tokenizer
    async fn load_model(&self) -> Result<MistralState, ConduitError> {
        log::info!("Loading Mistral model: {}", self.config.model_id);

        let device = self.config.device.to_device()?;
        log::info!("Using device: {:?}", self.config.device);

        // Resolve model files from local HuggingFace cache (no network access)
        let (model_paths, tokenizer_path, config_path) = self.resolve_cached_model_files()?;

        // Load tokenizer
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| ConduitError::LlmError(format!("Tokenizer load failed: {}", e)))?;

        // Get EOS token ID from tokenizer
        let eos_token_id = tokenizer.token_to_id("</s>").unwrap_or(2); // Mistral default EOS

        // Load model config
        let config_content = std::fs::read_to_string(&config_path)
            .map_err(|e| ConduitError::LlmError(format!("Failed to read config: {}", e)))?;
        let model_config: MistralModelConfig = serde_json::from_str(&config_content)
            .map_err(|e| ConduitError::LlmError(format!("Failed to parse config: {}", e)))?;

        // Load model weights (supports sharded safetensors)
        log::info!("Memory-mapping {} weight file(s)", model_paths.len());
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&model_paths, self.config.dtype.to_dtype(), &device)
                .map_err(|e| {
                    ConduitError::LlmError(format!("Failed to load model weights: {}", e))
                })?
        };

        let model = MistralModel::new(&model_config, vb)
            .map_err(|e| ConduitError::LlmError(format!("Failed to create model: {}", e)))?;

        log::info!("Mistral model loaded successfully");

        Ok(MistralState {
            model: RwLock::new(model),
            tokenizer,
            device,
            eos_token_id,
        })
    }

    /// Resolve model files from the local HuggingFace cache.
    ///
    /// This does NOT download anything — all downloads happen in the `prefetch`
    /// subcommand at deploy time.  If files are missing, we return a clear error
    /// telling the operator to run `dapple-prompt-conduit prefetch`.
    ///
    /// Returns (model_weight_paths, tokenizer_path, config_path).
    fn resolve_cached_model_files(&self) -> Result<(Vec<PathBuf>, PathBuf, PathBuf), ConduitError> {
        // HF cache layout:  $HF_HUB_CACHE/models--{org}--{repo}/snapshots/{rev}/
        let hub_cache = std::env::var("HF_HUB_CACHE").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
            format!("{}/.cache/huggingface/hub", home)
        });

        let model_dir_name = format!("models--{}", self.config.model_id.replace('/', "--"));
        let snapshots_dir = PathBuf::from(&hub_cache)
            .join(&model_dir_name)
            .join("snapshots");

        if !snapshots_dir.exists() {
            return Err(ConduitError::LlmError(format!(
                "Model cache not found at {}. Run `dapple-prompt-conduit prefetch` before starting the server.",
                snapshots_dir.display()
            )));
        }

        // Find the latest snapshot directory (there's typically one revision)
        let snapshot = std::fs::read_dir(&snapshots_dir)
            .map_err(|e| ConduitError::LlmError(format!("Cannot read snapshots dir: {}", e)))?
            .filter_map(|entry| entry.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
            .ok_or_else(|| {
                ConduitError::LlmError(format!(
                    "No snapshot found in {}. Run `dapple-prompt-conduit prefetch`.",
                    snapshots_dir.display()
                ))
            })?;

        let snap_dir = snapshot.path();
        log::info!("Using cached snapshot: {}", snap_dir.display());

        // Require config.json and tokenizer.json
        let config_path = snap_dir.join("config.json");
        let tokenizer_path = snap_dir.join("tokenizer.json");

        for (name, path) in [
            ("config.json", &config_path),
            ("tokenizer.json", &tokenizer_path),
        ] {
            if !path.exists() {
                return Err(ConduitError::LlmError(format!(
                    "{} not found at {}. Run `dapple-prompt-conduit prefetch`.",
                    name,
                    path.display()
                )));
            }
        }

        // Resolve weight files: sharded index or single file
        let index_path = snap_dir.join("model.safetensors.index.json");
        let single_path = snap_dir.join("model.safetensors");

        let model_paths = if index_path.exists() {
            // Sharded model — parse index to find shard filenames, then resolve
            // through symlinks (HF cache uses blobs/ with symlinks in snapshots/)
            let content = std::fs::read_to_string(&index_path).map_err(|e| {
                ConduitError::LlmError(format!("Failed to read {}: {}", index_path.display(), e))
            })?;
            let index: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
                ConduitError::LlmError(format!("Failed to parse index JSON: {}", e))
            })?;

            let mut shard_names: Vec<String> = index
                .get("weight_map")
                .and_then(|m| m.as_object())
                .map(|m| {
                    m.values()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            shard_names.sort();
            shard_names.dedup();

            if shard_names.is_empty() {
                return Err(ConduitError::LlmError(
                    "weight_map in index is empty. Model cache may be corrupt. Run `dapple-prompt-conduit prefetch`.".to_string()
                ));
            }

            log::info!("Sharded model: {} weight files", shard_names.len());
            let mut paths = Vec::with_capacity(shard_names.len());
            for name in &shard_names {
                let shard_path = snap_dir.join(name);
                if !shard_path.exists() {
                    return Err(ConduitError::LlmError(format!(
                        "Weight shard {} not found at {}. Run `dapple-prompt-conduit prefetch`.",
                        name,
                        shard_path.display()
                    )));
                }
                log::debug!("  shard: {}", shard_path.display());
                paths.push(shard_path);
            }
            paths
        } else if single_path.exists() {
            log::info!("Single-file model weights");
            vec![single_path]
        } else {
            return Err(ConduitError::LlmError(format!(
                "No model weights found in {}. Expected model.safetensors or model.safetensors.index.json. Run `dapple-prompt-conduit prefetch`.",
                snap_dir.display()
            )));
        };

        log::info!("Model files resolved: {} weight file(s)", model_paths.len());
        Ok((model_paths, tokenizer_path, config_path))
    }

    /// Format prompt using Mistral instruct template
    fn format_prompt(&self, user_message: &str, system_prompt: Option<&str>) -> String {
        match system_prompt {
            Some(sys) => format!("[INST] {}\n\n{} [/INST]", sys, user_message),
            None => format!("[INST] {} [/INST]", user_message),
        }
    }

    /// Encode text to token IDs
    fn encode(&self, state: &MistralState, text: &str) -> Result<Vec<u32>, ConduitError> {
        let encoding = state
            .tokenizer
            .encode(text, true)
            .map_err(|e| ConduitError::LlmError(format!("Tokenization failed: {}", e)))?;
        Ok(encoding.get_ids().to_vec())
    }

    /// Decode token ID to text
    fn decode(&self, state: &MistralState, token_id: u32) -> Result<String, ConduitError> {
        state
            .tokenizer
            .decode(&[token_id], false)
            .map_err(|e| ConduitError::LlmError(format!("Decoding failed: {}", e)))
    }
}

/// Sample next token from logits
fn sample_token(
    logits: &Tensor,
    params: &SamplingParams,
    rng: &mut impl rand::Rng,
) -> Result<u32, ConduitError> {
    let logits: Vec<f32> = logits
        .to_vec1()
        .map_err(|e| ConduitError::LlmError(format!("Failed to convert logits: {}", e)))?;

    // Greedy decoding if temperature is 0
    if params.temperature <= 0.0 {
        let (idx, _) = logits
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .ok_or_else(|| ConduitError::LlmError("Empty logits".to_string()))?;
        return Ok(idx as u32);
    }

    // Apply temperature
    let scaled: Vec<f32> = logits.iter().map(|&x| x / params.temperature).collect();

    // Apply top-k
    let mut indexed: Vec<(usize, f32)> = scaled.into_iter().enumerate().collect();
    indexed.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    indexed.truncate(params.top_k);

    // Softmax for probabilities
    let max_logit = indexed[0].1;
    let probs: Vec<f32> = indexed
        .iter()
        .map(|(_, logit)| (logit - max_logit).exp())
        .collect();
    let sum: f32 = probs.iter().sum();
    let probs: Vec<f32> = probs.iter().map(|p| p / sum).collect();

    // Apply top-p (nucleus sampling)
    let mut cumsum = 0.0;
    let mut nucleus_size = probs.len();
    for (i, p) in probs.iter().enumerate() {
        cumsum += p;
        if cumsum >= params.top_p {
            nucleus_size = i + 1;
            break;
        }
    }

    let probs = &probs[..nucleus_size];
    let indexed = &indexed[..nucleus_size];

    // Sample from distribution
    let dist = WeightedIndex::new(probs)
        .map_err(|e| ConduitError::LlmError(format!("Sampling distribution failed: {}", e)))?;
    let sampled_idx = dist.sample(rng);

    Ok(indexed[sampled_idx].0 as u32)
}

/// Check if generation should stop
fn should_stop(
    generated_tokens: &[u32],
    decoded_text: &str,
    params: &SamplingParams,
    eos_token_id: u32,
) -> bool {
    // Check max tokens
    if generated_tokens.len() >= params.max_tokens {
        return true;
    }

    // Check EOS token
    if let Some(&last) = generated_tokens.last() {
        if last == eos_token_id {
            return true;
        }
    }

    // Check stop sequences
    for stop in &params.stop_sequences {
        if decoded_text.ends_with(stop) {
            return true;
        }
    }

    false
}

/// Mistral backend using local Candle inference
pub struct MistralBackend {
    generator: Arc<Mutex<MistralGenerator>>,
}

impl MistralBackend {
    pub fn new(config: MistralConfig) -> Self {
        let generator = MistralGenerator::new(config);
        Self {
            generator: Arc::new(Mutex::new(generator)),
        }
    }

    /// Create from environment configuration
    pub fn from_env() -> Self {
        Self::new(MistralConfig::from_env())
    }
}

#[async_trait]
impl LlmBackend for MistralBackend {
    async fn execute_streaming(
        &self,
        prompt: &RenderedPrompt,
        model: &ModelConfig,
    ) -> Result<TokenStream, ConduitError> {
        let generator = self.generator.clone();

        // Extract system prompt and user message from rendered prompt
        let (system_prompt, user_message) = extract_prompt_parts(prompt);

        // Use model config for sampling params, with defaults for unspecified values
        let params = SamplingParams::from_model_config(model);

        // Initialize model and get input tokens
        let gen_guard = generator.lock().await;
        let state = gen_guard.get_state().await?;
        let formatted = gen_guard.format_prompt(&user_message, system_prompt.as_deref());
        let input_ids = gen_guard.encode(state, &formatted)?;
        let device = state.device.clone();
        let eos_token_id = state.eos_token_id;
        drop(gen_guard);

        // Create async stream that generates tokens
        let stream = stream::unfold(
            GenerationStreamState {
                generator,
                input_ids,
                params,
                device,
                eos_token_id,
                position: 0,
                generated_tokens: Vec::new(),
                decoded_text: String::new(),
                first_iteration: true,
            },
            |mut stream_state| async move {
                // Check if we should stop
                if should_stop(
                    &stream_state.generated_tokens,
                    &stream_state.decoded_text,
                    &stream_state.params,
                    stream_state.eos_token_id,
                ) {
                    return None;
                }

                // Generate the next token
                let result = generate_next_token(&mut stream_state).await;

                match result {
                    Ok(token_text) => Some((Ok(token_text), stream_state)),
                    Err(e) => Some((Err(e), stream_state)),
                }
            },
        );

        // Map token strings into StreamChunk::Token, then append a trailing
        // StreamChunk::Final at stream-end. Candle doesn't report provider token
        // counts, so usage is empty (the consumer falls back to its estimate).
        let stream = stream
            .map(|result| result.map(StreamChunk::Token))
            .chain(stream::once(async {
                Ok(StreamChunk::Final(UsageMetadata::default()))
            }));

        Ok(Box::pin(stream))
    }

    fn supports(&self, model: &ModelConfig) -> bool {
        // Support mistral lineage or self-hosted provider
        model.lineage_key == "mistral"
            || model.provider_key == "self-hosted"
            || model.lineage_key == "local"
    }

    async fn warmup(&self) -> Result<(), ConduitError> {
        log::info!("Warming up Mistral backend — loading model weights...");
        let gen_guard = self.generator.lock().await;
        gen_guard.get_state().await?;
        log::info!("Mistral backend warm — model ready for inference");
        Ok(())
    }
}

/// Generate the next token, updating the stream state
async fn generate_next_token(
    stream_state: &mut GenerationStreamState,
) -> Result<String, ConduitError> {
    let gen_guard = stream_state.generator.lock().await;
    let state = gen_guard.get_state().await?;

    // Prepare input tensor
    let input_tensor = if stream_state.first_iteration {
        // First iteration: process full input
        let t = Tensor::new(stream_state.input_ids.as_slice(), &stream_state.device)
            .map_err(|e| ConduitError::LlmError(e.to_string()))?;
        t.unsqueeze(0)
            .map_err(|e| ConduitError::LlmError(e.to_string()))?
    } else {
        // Subsequent iterations: single token
        let last_token = stream_state
            .generated_tokens
            .last()
            .copied()
            .ok_or_else(|| ConduitError::LlmError("No previous token".to_string()))?;
        let t = Tensor::new(&[last_token], &stream_state.device)
            .map_err(|e| ConduitError::LlmError(e.to_string()))?;
        t.unsqueeze(0)
            .map_err(|e| ConduitError::LlmError(e.to_string()))?
    };

    // Forward pass through model (requires write lock for KV cache)
    let logits = {
        let mut model_guard = state.model.write().await;
        model_guard
            .forward(&input_tensor, stream_state.position)
            .map_err(|e| ConduitError::LlmError(format!("Forward pass failed: {}", e)))?
    };

    // Get logits for last position
    let seq_len = logits
        .dim(1)
        .map_err(|e| ConduitError::LlmError(e.to_string()))?;

    let last_logits = logits
        .i((.., seq_len - 1, ..))
        .map_err(|e| ConduitError::LlmError(e.to_string()))?
        .squeeze(1)
        .map_err(|e| ConduitError::LlmError(e.to_string()))?
        .squeeze(0)
        .map_err(|e| ConduitError::LlmError(e.to_string()))?;

    // Sample next token
    let seed = stream_state.params.seed.unwrap_or_else(rand::random);
    let mut rng = rand::rngs::StdRng::seed_from_u64(
        seed.wrapping_add(stream_state.generated_tokens.len() as u64),
    );
    let token = sample_token(&last_logits, &stream_state.params, &mut rng)?;

    // Decode token to text
    let token_text = gen_guard.decode(state, token)?;

    // Update state
    let input_len = if stream_state.first_iteration {
        stream_state.input_ids.len()
    } else {
        1
    };
    stream_state.position += input_len;
    stream_state.generated_tokens.push(token);
    stream_state.decoded_text.push_str(&token_text);
    stream_state.first_iteration = false;

    Ok(token_text)
}

/// State for the generation stream
struct GenerationStreamState {
    generator: Arc<Mutex<MistralGenerator>>,
    input_ids: Vec<u32>,
    params: SamplingParams,
    device: Device,
    eos_token_id: u32,
    position: usize,
    generated_tokens: Vec<u32>,
    decoded_text: String,
    first_iteration: bool,
}

/// Extract system prompt and user message from RenderedPrompt
fn extract_prompt_parts(prompt: &RenderedPrompt) -> (Option<String>, String) {
    let mut system_prompt = None;
    let mut user_messages = Vec::new();

    for msg in &prompt.messages {
        match msg.role.as_deref() {
            Some("system") => {
                system_prompt = Some(msg.content.clone());
            }
            Some("user") | Some("human") | None => {
                user_messages.push(msg.content.clone());
            }
            Some("assistant") | Some("ai") => {
                // Include assistant messages in context
                user_messages.push(format!("Assistant: {}", msg.content));
            }
            _ => {
                user_messages.push(msg.content.clone());
            }
        }
    }

    let user_message = user_messages.join("\n\n");
    (system_prompt, user_message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mistral_config_default() {
        let config = MistralConfig::default();
        assert_eq!(config.model_id, "mistralai/Mistral-7B-Instruct-v0.3");
        assert_eq!(config.max_context_length, 8192);
    }

    #[test]
    fn test_device_config_cpu() {
        let device_config = DeviceConfig::Cpu;
        let device = device_config.to_device().unwrap();
        assert!(matches!(device, Device::Cpu));
    }

    #[test]
    fn test_sampling_params_default() {
        let params = SamplingParams::default();
        assert_eq!(params.temperature, 0.7);
        assert_eq!(params.top_p, 0.9);
        assert_eq!(params.top_k, 40);
        assert_eq!(params.max_tokens, 2048);
    }

    #[test]
    fn test_format_prompt_with_system() {
        let generator = MistralGenerator::new(MistralConfig::default());
        let formatted =
            generator.format_prompt("Hello, how are you?", Some("You are a helpful assistant."));
        assert!(formatted.contains("[INST]"));
        assert!(formatted.contains("You are a helpful assistant."));
        assert!(formatted.contains("Hello, how are you?"));
        assert!(formatted.contains("[/INST]"));
    }

    #[test]
    fn test_format_prompt_without_system() {
        let generator = MistralGenerator::new(MistralConfig::default());
        let formatted = generator.format_prompt("Hello, how are you?", None);
        assert!(formatted.contains("[INST]"));
        assert!(formatted.contains("Hello, how are you?"));
        assert!(formatted.contains("[/INST]"));
    }

    #[test]
    fn test_should_stop_max_tokens() {
        let params = SamplingParams {
            max_tokens: 5,
            ..Default::default()
        };
        let tokens = vec![1, 2, 3, 4, 5];
        assert!(should_stop(&tokens, "hello", &params, 2));
    }

    #[test]
    fn test_should_stop_eos_token() {
        let params = SamplingParams::default();
        let tokens = vec![1, 2];
        assert!(should_stop(&tokens, "hello", &params, 2));
    }

    #[test]
    fn test_should_stop_stop_sequence() {
        let params = SamplingParams {
            stop_sequences: vec!["[/INST]".to_string()],
            ..Default::default()
        };
        let tokens = vec![1];
        assert!(should_stop(&tokens, "hello [/INST]", &params, 999));
    }

    #[test]
    fn test_backend_supports() {
        let backend = MistralBackend::new(MistralConfig::default());

        let mistral_model = ModelConfig {
            lineage_key: "mistral".to_string(),
            provider_key: "mistral".to_string(),
            tier: "7b-instruct".to_string(),
            ..Default::default()
        };
        assert!(backend.supports(&mistral_model));

        let self_hosted_model = ModelConfig {
            lineage_key: "llama".to_string(),
            provider_key: "self-hosted".to_string(),
            tier: "7b-q5".to_string(),
            ..Default::default()
        };
        assert!(backend.supports(&self_hosted_model));

        let anthropic_model = ModelConfig {
            lineage_key: "claude-3-5".to_string(),
            provider_key: "anthropic".to_string(),
            tier: "sonnet".to_string(),
            ..Default::default()
        };
        assert!(!backend.supports(&anthropic_model));
    }

    #[test]
    fn test_extract_prompt_parts() {
        let prompt = RenderedPrompt {
            prompt_name: "test".to_string(),
            messages: vec![
                crate::service::template::PromptMessage {
                    role: Some("system".to_string()),
                    content: "You are helpful.".to_string(),
                },
                crate::service::template::PromptMessage {
                    role: Some("user".to_string()),
                    content: "Hello!".to_string(),
                },
            ],
        };

        let (system, user) = extract_prompt_parts(&prompt);
        assert_eq!(system, Some("You are helpful.".to_string()));
        assert_eq!(user, "Hello!");
    }

    #[test]
    fn test_sampling_params_from_model_config_all_specified() {
        let model = ModelConfig {
            lineage_key: "mistral".to_string(),
            provider_key: "self-hosted".to_string(),
            tier: "7b".to_string(),
            temperature: Some(0.5),
            top_p: Some(0.8),
            top_k: Some(50),
            max_tokens: Some(1024),
            ..Default::default()
        };

        let params = SamplingParams::from_model_config(&model);
        assert_eq!(params.temperature, 0.5);
        assert_eq!(params.top_p, 0.8);
        assert_eq!(params.top_k, 50);
        assert_eq!(params.max_tokens, 1024);
    }

    #[test]
    fn test_sampling_params_from_model_config_partial() {
        let model = ModelConfig {
            lineage_key: "mistral".to_string(),
            provider_key: "self-hosted".to_string(),
            tier: "7b".to_string(),
            temperature: Some(0.3),
            top_p: None, // Use default
            top_k: None, // Use default
            max_tokens: Some(512),
            ..Default::default()
        };

        let params = SamplingParams::from_model_config(&model);
        assert_eq!(params.temperature, 0.3);
        assert_eq!(params.top_p, 0.9); // Default
        assert_eq!(params.top_k, 40); // Default
        assert_eq!(params.max_tokens, 512);
    }

    #[test]
    fn test_sampling_params_from_model_config_none() {
        let model = ModelConfig::default();

        let params = SamplingParams::from_model_config(&model);
        let defaults = SamplingParams::default();
        assert_eq!(params.temperature, defaults.temperature);
        assert_eq!(params.top_p, defaults.top_p);
        assert_eq!(params.top_k, defaults.top_k);
        assert_eq!(params.max_tokens, defaults.max_tokens);
    }
}
