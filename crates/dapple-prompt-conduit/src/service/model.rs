//! Model configuration service for Conduit
//!
//! This module handles runtime model selection based on InferenceProfiles.
//! The model taxonomy provides a structured way to think about AI models:
//!
//!   Provider  -> Billing/API concern (Anthropic, OpenAI, Self-Hosted)
//!   Lineage   -> Prompting behavior/style (claude-3-5, vits, flan-t5)
//!   Tier      -> Model variant within a lineage (sonnet, opus, ljspeech)
//!   Release   -> Official version timestamp (20241022, v0.1)
//!   Variant   -> Custom fine-tunes that branch from a Release
//!
//! Model configuration comes from Journey's InferenceProfiles, providing
//! flexibility for A/B testing and multi-provider deployments.

use dapple_data::model::model_release::build_release_chain;
use dapple_data::query::inference_profiles::select_model_for_dispatch_prompt;
use dapple_data::DbPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::error::ConduitError;
use crate::service::llm::LlmConfig;

/// Configuration for a model
///
/// This struct contains all the information needed to:
/// 1. Build the release chain for template resolution
/// 2. Route requests to the correct backend (via lineage_key)
/// 3. Apply sampling parameters
#[derive(Debug, Clone)]
pub struct ModelConfig {
    /// The lineage key (prompting paradigm) - e.g., "claude-3-5", "vits", "flan-t5"
    /// Used for routing: "vits" -> TTS, "flan-t5" -> summarization, "modernbert" -> MLM
    pub lineage_key: String,
    /// The provider key (hosting endpoint) - e.g., "anthropic", "self-hosted"
    pub provider_key: String,
    /// The tier within the lineage - e.g., "sonnet", "ljspeech", "base-samsum"
    pub tier: String,
    /// The release version - e.g., "20241022", "v0.1"
    pub release: Option<String>,
    /// If this is a variant (fine-tune), the variant ID
    pub variant_id: Option<Uuid>,
    /// Priority from InferenceProfileConfig (lower = preferred)
    pub priority: i32,

    // Sampling parameters (optional, backend uses defaults if None)
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<usize>,
    pub max_tokens: Option<usize>,
}

impl ModelConfig {
    /// Build the release chain for template resolution
    ///
    /// Returns an array like:
    /// `["@provider:lineage/tier:release", "@provider:lineage/tier", "lineage/tier:release", "lineage/tier", "lineage"]`
    pub fn build_chain(&self) -> Vec<String> {
        build_release_chain(
            &self.lineage_key,
            &self.tier,
            self.release.as_deref(),
            &self.provider_key,
        )
    }

    /// Build the engine key for compatibility with existing template resolution
    /// Format: "lineage-tier" (e.g., "vits-ljspeech")
    pub fn engine_key(&self) -> String {
        format!("{}-{}", self.lineage_key, self.tier)
    }
}

impl Default for ModelConfig {
    /// Returns the default stub model configuration
    fn default() -> Self {
        Self {
            lineage_key: "stub".to_string(),
            provider_key: "stub".to_string(),
            tier: "default".to_string(),
            release: None,
            variant_id: None,
            priority: 0,
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: None,
        }
    }
}

/// Service for resolving model configuration
///
/// In the model taxonomy architecture, model configuration comes from:
/// 1. Journey's InferenceProfiles (via journey_inference_profiles)
/// 2. InferenceProfileConfigs that map prompts to models
/// 3. Runtime selection based on availability/health/credits
pub struct ModelService {
    pool: Arc<DbPool>,
    llm_config: LlmConfig,
}

impl ModelService {
    pub fn new(pool: Arc<DbPool>, llm_config: LlmConfig) -> Self {
        Self { pool, llm_config }
    }

    /// Resolve the model configuration for a prompt execution.
    ///
    /// Prefers the journey's authored inference profile: when `dispatch_id` is
    /// present and the journey has an `inference_profile_config` for this prompt,
    /// that model wins. This is what makes a guide's `:::inference-profile`
    /// load-bearing at runtime. Otherwise (no dispatch, no matching config, or a
    /// transient lookup error) it falls back to the environment-based default —
    /// preserving prior behavior for every prompt without an authored profile.
    pub async fn resolve_model_for_prompt(
        &self,
        dispatch_id: Option<Uuid>,
        prompt_object_id: Uuid,
    ) -> Result<ModelConfig, ConduitError> {
        if let Some(did) = dispatch_id {
            match select_model_for_dispatch_prompt(self.pool.as_ref(), did, prompt_object_id).await
            {
                Ok(Some(row)) => {
                    log::debug!(
                        "Resolved model from inference profile: lineage={}, provider={}, \
                         tier={}, release={:?} (dispatch={}, prompt={})",
                        row.lineage_key,
                        row.provider_key,
                        row.tier,
                        row.release,
                        did,
                        prompt_object_id
                    );
                    return Ok(ModelConfig {
                        lineage_key: row.lineage_key,
                        provider_key: row.provider_key,
                        tier: row.tier,
                        release: row.release,
                        variant_id: None,
                        priority: 0,
                        temperature: None,
                        top_p: None,
                        top_k: None,
                        max_tokens: Some(8192),
                    });
                }
                Ok(None) => {
                    log::debug!(
                        "No inference-profile config for prompt {} in dispatch {}; using env default",
                        prompt_object_id,
                        did
                    );
                }
                Err(e) => {
                    log::warn!(
                        "Inference-profile resolution failed for prompt {} (dispatch {}): {}; \
                         using env default",
                        prompt_object_id,
                        did,
                        e
                    );
                }
            }
        }
        self.default_model_from_env()
    }

    /// Environment-based default model: Anthropic (API key, fast) > mistral.rs
    /// (local GGUF) > Candle Mistral (legacy) > error. The fallback used when no
    /// authored inference profile applies to the prompt.
    fn default_model_from_env(&self) -> Result<ModelConfig, ConduitError> {
        if self.llm_config.anthropic_api_key.is_some() {
            log::debug!("Using Anthropic Claude backend (env default)");
            return Ok(ModelConfig {
                lineage_key: "claude-4-6".to_string(),
                provider_key: "anthropic".to_string(),
                tier: "sonnet".to_string(),
                release: Some("20250929".to_string()),
                variant_id: None,
                priority: 0,
                temperature: None,
                top_p: None,
                top_k: None,
                max_tokens: Some(8192),
            });
        }
        if self.llm_config.mistralrs_model_id.is_some() {
            log::debug!("Using mistral.rs local backend (env default)");
            return Ok(ModelConfig {
                lineage_key: "mistral".to_string(),
                provider_key: "self-hosted".to_string(),
                tier: "7b-instruct".to_string(),
                release: Some("v0.3".to_string()),
                variant_id: None,
                priority: 0,
                temperature: Some(0.7),
                top_p: Some(0.9),
                top_k: Some(40),
                max_tokens: Some(2048),
            });
        }
        if self.llm_config.hf_token.is_some() {
            log::debug!("Using Mistral Candle backend (legacy, env default)");
            return Ok(ModelConfig {
                lineage_key: "mistral".to_string(),
                provider_key: "self-hosted".to_string(),
                tier: "7b-instruct".to_string(),
                release: Some("v0.3".to_string()),
                variant_id: None,
                priority: 0,
                temperature: Some(0.7),
                top_p: Some(0.9),
                top_k: Some(40),
                max_tokens: Some(2048),
            });
        }
        Err(ConduitError::LlmError(
            "No LLM backend available. Set ANTHROPIC_API_KEY for Claude or HF_TOKEN for Mistral."
                .to_string(),
        ))
    }
}
