//! VITS TTS backend using ONNX Runtime.
//!
//! VITS (Variational Inference with adversarial learning for end-to-end Text-to-Speech)
//! is a lightweight, fast TTS model that works well on CPU and across platforms.
//!
//! Expected model structure at `{ONNX_MODEL_PATH}/{model_name}/`:
//! - `model.onnx` - The VITS model
//! - `tokens.txt` - Token vocabulary (character/phoneme to ID mapping)
//! - `lexicon.txt` - Optional word-to-phoneme mapping
//!
//! Supported models:
//! - `vits-ljspeech` - English single speaker (22050 Hz)
//! - `vits-vctk` - English multi-speaker, 109 voices (22050 Hz)
//! - `vits-melo-zh-en` - Multilingual Chinese + English (44100 Hz)

use super::{SynthesizeRequest, SynthesizeResult, TtsBackend, TtsConfig};
use crate::error::OnnxServiceError;
use crate::service::onnx::OnnxBackend;
use async_trait::async_trait;
use ndarray::Array2;
use ort::session::Session;
use ort::value::TensorRef;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Default sample rate for VITS models (most use 22050 Hz)
const DEFAULT_SAMPLE_RATE: u32 = 22050;

/// VITS TTS backend using ONNX Runtime
pub struct VitsTtsBackend {
    config: TtsConfig,
    model_name: String,
    onnx_backend: OnnxBackend,
    /// Lazy-loaded model state
    state: Arc<Mutex<Option<VitsState>>>,
}

struct VitsState {
    session: Session,
    tokenizer: VitsTokenizer,
    sample_rate: u32,
    speaker_id: Option<i64>,
    #[allow(dead_code)]
    supported_languages: Vec<String>,
}

/// Simple tokenizer for VITS models
/// Maps characters/phonemes to token IDs based on tokens.txt
struct VitsTokenizer {
    token_to_id: HashMap<String, i64>,
    /// Special tokens
    #[allow(dead_code)]
    pad_id: i64,
    eos_id: i64,
    unk_id: i64,
}

impl VitsTokenizer {
    /// Load tokenizer from tokens.txt file
    ///
    /// Format: one token per line, ID is the line number (0-indexed)
    /// Special tokens are typically at the start: <pad>, <eos>, <unk>, etc.
    fn from_tokens_file(path: &Path) -> Result<Self, OnnxServiceError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            OnnxServiceError::TtsError(format!(
                "Failed to read tokens.txt from {}: {}",
                path.display(),
                e
            ))
        })?;

        let mut token_to_id = HashMap::new();
        let mut pad_id = 0i64;
        let mut eos_id = 1i64;
        let mut unk_id = 2i64;

        for (id, line) in content.lines().enumerate() {
            let token = line.trim();
            if token.is_empty() {
                continue;
            }

            let id = id as i64;
            token_to_id.insert(token.to_string(), id);

            // Detect special tokens
            match token {
                "<pad>" | "[PAD]" | "_" => pad_id = id,
                "<eos>" | "[EOS]" | "</s>" => eos_id = id,
                "<unk>" | "[UNK]" => unk_id = id,
                _ => {}
            }
        }

        log::debug!(
            "Loaded VITS tokenizer with {} tokens (pad={}, eos={}, unk={})",
            token_to_id.len(),
            pad_id,
            eos_id,
            unk_id
        );

        Ok(Self {
            token_to_id,
            pad_id,
            eos_id,
            unk_id,
        })
    }

    /// Encode text to token IDs
    ///
    /// VITS models typically use character-level tokenization with some preprocessing
    fn encode(&self, text: &str) -> Vec<i64> {
        let mut ids = Vec::new();

        // Normalize text: lowercase, basic punctuation handling
        let normalized = text.to_lowercase();

        for ch in normalized.chars() {
            let token = ch.to_string();
            if let Some(&id) = self.token_to_id.get(&token) {
                ids.push(id);
            } else if ch.is_whitespace() {
                // Try space token variants
                if let Some(&id) = self.token_to_id.get(" ") {
                    ids.push(id);
                } else if let Some(&id) = self.token_to_id.get("<space>") {
                    ids.push(id);
                }
            } else {
                // Unknown character - use unk token
                ids.push(self.unk_id);
            }
        }

        // Add EOS token
        ids.push(self.eos_id);

        ids
    }
}

/// Model metadata loaded from config.json or inferred from model name
#[derive(Debug, Clone)]
struct VitsModelConfig {
    sample_rate: u32,
    num_speakers: Option<usize>,
    languages: Vec<String>,
}

impl VitsModelConfig {
    fn from_model_dir(model_dir: &Path, model_name: &str) -> Self {
        // Try to load from config.json
        let config_path = model_dir.join("config.json");
        if config_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    let sample_rate = json
                        .get("audio")
                        .and_then(|a| a.get("sample_rate"))
                        .or_else(|| json.get("sampling_rate"))
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32)
                        .unwrap_or(DEFAULT_SAMPLE_RATE);

                    let num_speakers = json
                        .get("num_speakers")
                        .or_else(|| json.get("n_speakers"))
                        .and_then(|v| v.as_u64())
                        .map(|v| v as usize);

                    let languages = json
                        .get("languages")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_else(|| Self::infer_languages(model_name));

                    return Self {
                        sample_rate,
                        num_speakers,
                        languages,
                    };
                }
            }
        }

        // Fall back to inference from model name
        Self::from_model_name(model_name)
    }

    fn from_model_name(model_name: &str) -> Self {
        let name_lower = model_name.to_lowercase();

        // Infer sample rate
        let sample_rate = if name_lower.contains("melo") {
            44100 // MeloTTS uses 44100 Hz
        } else {
            DEFAULT_SAMPLE_RATE
        };

        // Infer number of speakers
        let num_speakers = if name_lower.contains("vctk") {
            Some(109)
        } else if name_lower.contains("melo") {
            Some(1) // MeloTTS has speaker embeddings but typically single output
        } else {
            None // Single speaker
        };

        // Infer languages
        let languages = Self::infer_languages(model_name);

        Self {
            sample_rate,
            num_speakers,
            languages,
        }
    }

    fn infer_languages(model_name: &str) -> Vec<String> {
        let name_lower = model_name.to_lowercase();

        if name_lower.contains("zh") && name_lower.contains("en") {
            vec!["zh".to_string(), "en".to_string()]
        } else if name_lower.contains("zh") {
            vec!["zh".to_string()]
        } else if name_lower.contains("ljspeech") || name_lower.contains("vctk") {
            vec!["en".to_string()]
        } else {
            // Default to English
            vec!["en".to_string()]
        }
    }
}

impl VitsTtsBackend {
    /// Eagerly load the ONNX model so OOM or missing-model errors surface at startup.
    pub async fn warmup(&self) -> Result<(), OnnxServiceError> {
        let mut state_guard = self.state.lock().await;
        if state_guard.is_none() {
            *state_guard = Some(self.load_model().await?);
        }
        Ok(())
    }

    /// Create a new VITS TTS backend (model is loaded lazily on first use)
    pub async fn new(config: &TtsConfig) -> Result<Self, OnnxServiceError> {
        let onnx_backend = OnnxBackend::new().map_err(|e| {
            OnnxServiceError::TtsError(format!("Failed to initialize ONNX Runtime: {}", e))
        })?;

        // Use model name directly (e.g., "vits-ljspeech")
        let model_name = config.model_id.clone();

        Ok(Self {
            config: config.clone(),
            model_name,
            onnx_backend,
            state: Arc::new(Mutex::new(None)),
        })
    }

    /// Load the model and tokenizer (called lazily on first use)
    async fn load_model(&self) -> Result<VitsState, OnnxServiceError> {
        log::info!(
            "Loading VITS ONNX model: {} with {}",
            self.model_name,
            self.onnx_backend.execution_provider()
        );

        let model_dir = self.onnx_backend.model_base_path().join(&self.model_name);

        // Check if model exists
        if !self.onnx_backend.model_exists(&self.model_name) {
            return Err(OnnxServiceError::TtsError(format!(
                "VITS ONNX model not found at {}. Download with: python scripts/onnx_convert/download_vits.py --model {} --output-dir {}",
                model_dir.display(),
                self.model_name,
                model_dir.display()
            )));
        }

        // Load ONNX session
        let session = self
            .onnx_backend
            .load_model(&self.model_name)
            .map_err(|e| OnnxServiceError::TtsError(format!("Failed to load ONNX model: {}", e)))?;

        // Load tokenizer from tokens.txt
        let tokens_path = model_dir.join("tokens.txt");
        let tokenizer = VitsTokenizer::from_tokens_file(&tokens_path)?;

        // Load model config
        let model_config = VitsModelConfig::from_model_dir(&model_dir, &self.model_name);

        // Parse speaker ID from config
        let speaker_id = self.config.speaker_id;

        // Validate speaker ID if model has multiple speakers
        if let Some(num_speakers) = model_config.num_speakers {
            if let Some(sid) = speaker_id {
                if sid as usize >= num_speakers {
                    return Err(OnnxServiceError::TtsError(format!(
                        "Speaker ID {} is out of range for model {} (max: {})",
                        sid,
                        self.model_name,
                        num_speakers - 1
                    )));
                }
            }
        }

        // Validate language if specified
        if let Some(ref lang) = self.config.language {
            if !model_config.languages.contains(lang) {
                return Err(OnnxServiceError::TtsError(format!(
                    "Language '{}' not supported by model {} (supports: {})",
                    lang,
                    self.model_name,
                    model_config.languages.join(", ")
                )));
            }
        }

        log::info!(
            "VITS ONNX model loaded successfully (sample_rate={}, languages={:?}, speakers={:?})",
            model_config.sample_rate,
            model_config.languages,
            model_config.num_speakers
        );

        Ok(VitsState {
            session,
            tokenizer,
            sample_rate: model_config.sample_rate,
            speaker_id,
            supported_languages: model_config.languages,
        })
    }

    /// Generate audio using the VITS model with optional speaker_id override
    fn generate_audio_with_speaker(
        &self,
        state: &mut VitsState,
        text: &str,
        speaker_id: Option<i64>,
    ) -> Result<Vec<u8>, OnnxServiceError> {
        // Tokenize the text
        let token_ids = state.tokenizer.encode(text);
        let seq_len = token_ids.len();

        log::debug!("VITS tokenized {} chars -> {} tokens", text.len(), seq_len);

        // Create input tensors
        // VITS expects: input_ids [1, seq_len], input_lengths [1]
        let input_ids_array = Array2::from_shape_vec((1, seq_len), token_ids).map_err(|e| {
            OnnxServiceError::TtsError(format!("Failed to create input_ids array: {}", e))
        })?;

        let input_lengths: Vec<i64> = vec![seq_len as i64];
        let input_lengths_array = ndarray::Array1::from_vec(input_lengths);

        // Create tensor refs
        let input_ids_tensor = TensorRef::from_array_view(input_ids_array.view()).map_err(|e| {
            OnnxServiceError::TtsError(format!("Failed to create input_ids tensor: {}", e))
        })?;
        let input_lengths_tensor =
            TensorRef::from_array_view(input_lengths_array.view()).map_err(|e| {
                OnnxServiceError::TtsError(format!("Failed to create input_lengths tensor: {}", e))
            })?;

        // Build inputs based on model type
        let outputs = if let Some(speaker_id) = speaker_id {
            // Multi-speaker model
            let sid_array = ndarray::Array1::from_vec(vec![speaker_id]);
            let sid_tensor = TensorRef::from_array_view(sid_array.view()).map_err(|e| {
                OnnxServiceError::TtsError(format!("Failed to create sid tensor: {}", e))
            })?;

            // Add noise scales for variability (typical VITS params)
            let noise_scale = ndarray::Array1::from_vec(vec![0.667f32]);
            let noise_scale_w = ndarray::Array1::from_vec(vec![0.8f32]);
            let length_scale = ndarray::Array1::from_vec(vec![1.0f32]);

            let noise_scale_tensor =
                TensorRef::from_array_view(noise_scale.view()).map_err(|e| {
                    OnnxServiceError::TtsError(format!(
                        "Failed to create noise_scale tensor: {}",
                        e
                    ))
                })?;
            let noise_scale_w_tensor =
                TensorRef::from_array_view(noise_scale_w.view()).map_err(|e| {
                    OnnxServiceError::TtsError(format!(
                        "Failed to create noise_scale_w tensor: {}",
                        e
                    ))
                })?;
            let length_scale_tensor =
                TensorRef::from_array_view(length_scale.view()).map_err(|e| {
                    OnnxServiceError::TtsError(format!(
                        "Failed to create length_scale tensor: {}",
                        e
                    ))
                })?;

            state
                .session
                .run(ort::inputs![
                    "input" => input_ids_tensor,
                    "input_lengths" => input_lengths_tensor,
                    "scales" => noise_scale_tensor,
                    "scales_w" => noise_scale_w_tensor,
                    "length_scale" => length_scale_tensor,
                    "sid" => sid_tensor
                ])
                .map_err(|e| OnnxServiceError::TtsError(format!("ONNX inference failed: {}", e)))?
        } else {
            // Single-speaker model with noise/length scales (sherpa-onnx style)
            // Most VITS ONNX exports use this format
            let noise_scale = ndarray::Array1::from_vec(vec![0.667f32]);
            let noise_scale_w = ndarray::Array1::from_vec(vec![0.8f32]);
            let length_scale = ndarray::Array1::from_vec(vec![1.0f32]);

            let noise_scale_tensor =
                TensorRef::from_array_view(noise_scale.view()).map_err(|e| {
                    OnnxServiceError::TtsError(format!(
                        "Failed to create noise_scale tensor: {}",
                        e
                    ))
                })?;
            let noise_scale_w_tensor =
                TensorRef::from_array_view(noise_scale_w.view()).map_err(|e| {
                    OnnxServiceError::TtsError(format!(
                        "Failed to create noise_scale_w tensor: {}",
                        e
                    ))
                })?;
            let length_scale_tensor =
                TensorRef::from_array_view(length_scale.view()).map_err(|e| {
                    OnnxServiceError::TtsError(format!(
                        "Failed to create length_scale tensor: {}",
                        e
                    ))
                })?;

            state
                .session
                .run(ort::inputs![
                    "input" => input_ids_tensor,
                    "input_lengths" => input_lengths_tensor,
                    "scales" => noise_scale_tensor,
                    "scales_w" => noise_scale_w_tensor,
                    "length_scale" => length_scale_tensor
                ])
                .map_err(|e| OnnxServiceError::TtsError(format!("ONNX inference failed: {}", e)))?
        };

        // Extract audio output
        let audio_tensor = if outputs.len() == 0 {
            return Err(OnnxServiceError::TtsError(
                "No audio output from model".to_string(),
            ));
        } else {
            // Try common output names
            outputs
                .get("output")
                .or_else(|| outputs.get("audio"))
                .or_else(|| outputs.get("wav"))
                .unwrap_or(&outputs[0])
        };

        let audio: ndarray::ArrayViewD<'_, f32> =
            audio_tensor.try_extract_array::<f32>().map_err(|e| {
                OnnxServiceError::TtsError(format!("Failed to extract audio tensor: {}", e))
            })?;

        // Flatten to 1D samples
        let samples: Vec<f32> = audio.iter().cloned().collect();

        // Normalize audio
        let mut normalized = samples;
        normalize_audio_peak(&mut normalized, 0.95);

        log::debug!(
            "Generated {} PCM samples at {} Hz",
            normalized.len(),
            state.sample_rate
        );

        // Encode as WAV
        let mut wav_bytes = Vec::new();
        write_pcm_as_wav(&mut wav_bytes, &normalized, state.sample_rate)
            .map_err(|e| OnnxServiceError::TtsError(format!("WAV encoding failed: {}", e)))?;

        Ok(wav_bytes)
    }
}

#[async_trait]
impl TtsBackend for VitsTtsBackend {
    async fn synthesize(
        &self,
        request: &SynthesizeRequest,
    ) -> Result<SynthesizeResult, OnnxServiceError> {
        // Acquire lock and lazily initialize model
        let mut state_guard = self.state.lock().await;
        if state_guard.is_none() {
            *state_guard = Some(self.load_model().await?);
        }
        let state = state_guard.as_mut().unwrap();

        // Override speaker_id from request if provided
        let effective_speaker_id = request.speaker_id.or(state.speaker_id);

        log::info!(
            "Synthesizing {} characters with VITS ONNX (speaker_id={:?}, language={:?})",
            request.text.len(),
            effective_speaker_id,
            request.language
        );

        let audio_bytes =
            self.generate_audio_with_speaker(state, &request.text, effective_speaker_id)?;

        Ok(SynthesizeResult::new(
            audio_bytes,
            state.sample_rate,
            request.text.clone(),
        ))
    }

    fn name(&self) -> &str {
        "vits-onnx"
    }

    fn sample_rate(&self) -> u32 {
        DEFAULT_SAMPLE_RATE
    }

    async fn warmup(&self) -> Result<(), OnnxServiceError> {
        VitsTtsBackend::warmup(self).await
    }
}

/// Normalize audio to target peak level while preserving dynamic range
fn normalize_audio_peak(samples: &mut [f32], target_peak: f32) {
    if samples.is_empty() {
        return;
    }

    // Find the current peak (max absolute value)
    let current_peak = samples
        .iter()
        .map(|s| s.abs())
        .fold(0.0f32, |a, b| a.max(b));

    // Avoid division by zero for silent audio
    if current_peak < 1e-6 {
        return;
    }

    // Calculate scale factor to reach target peak
    let scale = target_peak / current_peak;

    // Apply scaling to all samples
    for sample in samples.iter_mut() {
        *sample *= scale;
    }
}

/// Write PCM samples as WAV format
fn write_pcm_as_wav<W: Write>(w: &mut W, samples: &[f32], sample_rate: u32) -> std::io::Result<()> {
    let len = 12u32; // header
    let len = len + 24u32; // fmt
    let len = len + samples.len() as u32 * 2 + 8; // data
    let n_channels = 1u16;
    let bytes_per_second = sample_rate * 2 * n_channels as u32;

    w.write_all(b"RIFF")?;
    w.write_all(&(len - 8).to_le_bytes())?;
    w.write_all(b"WAVE")?;

    // Format block
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&n_channels.to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&bytes_per_second.to_le_bytes())?;
    w.write_all(&2u16.to_le_bytes())?; // bytes per sample
    w.write_all(&16u16.to_le_bytes())?; // bits per sample

    // Data block
    w.write_all(b"data")?;
    w.write_all(&(samples.len() as u32 * 2).to_le_bytes())?;
    for sample in samples.iter() {
        let i16_sample = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
        w.write_all(&i16_sample.to_le_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wav_encoding() {
        let samples = vec![0.0f32, 0.5, -0.5, 1.0, -1.0];
        let mut wav_bytes = Vec::new();
        write_pcm_as_wav(&mut wav_bytes, &samples, 22050).unwrap();

        // Check WAV header
        assert!(wav_bytes.len() > 44);
        assert_eq!(&wav_bytes[0..4], b"RIFF");
        assert_eq!(&wav_bytes[8..12], b"WAVE");
    }

    #[test]
    fn test_peak_normalization() {
        let mut samples = vec![0.0, 0.25, -0.5, 0.3, -0.1];
        normalize_audio_peak(&mut samples, 0.95);

        let peak = samples
            .iter()
            .map(|s| s.abs())
            .fold(0.0f32, |a, b| a.max(b));
        assert!((peak - 0.95).abs() < 0.001);
    }

    #[test]
    fn test_model_config_inference() {
        let config = VitsModelConfig::from_model_name("vits-ljspeech");
        assert_eq!(config.sample_rate, 22050);
        assert!(config.num_speakers.is_none());
        assert_eq!(config.languages, vec!["en"]);

        let config = VitsModelConfig::from_model_name("vits-vctk");
        assert_eq!(config.sample_rate, 22050);
        assert_eq!(config.num_speakers, Some(109));
        assert_eq!(config.languages, vec!["en"]);

        let config = VitsModelConfig::from_model_name("vits-melo-zh-en");
        assert_eq!(config.sample_rate, 44100);
        assert_eq!(config.languages, vec!["zh", "en"]);
    }
}
