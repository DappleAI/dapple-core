//! Model Release models for specific model configurations
//!
//! A ModelRelease represents a specific model version at a specific provider.
//! It combines:
//!   - Lineage: The prompting paradigm (claude-3-5, gpt-4, mistral, vits)
//!   - Provider: Where the model is hosted (anthropic, self-hosted, nvidia-nim)
//!   - Tier: The capability level within the lineage (sonnet, opus, 7b, ljspeech)
//!   - Release: The official version timestamp (20241022, v0.1)
//!
//! The same tier+release can exist at multiple providers (e.g., mistral-7b at
//! both Mistral AI and self-hosted).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::DateTime;

/// Internal model for model_release from database query
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelReleaseModel {
    pub id: Uuid,
    pub lineage_id: i16,
    pub lineage_key: String,
    pub provider_id: i16,
    pub provider_key: String,
    pub tier: String,
    pub release: Option<String>,
    pub created_at: OffsetDateTime,
}

/// Public DTO for model release
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct ModelRelease {
    pub id: Uuid,
    pub lineage_key: String,
    pub provider_key: String,
    pub tier: String,
    pub release: Option<String>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl From<ModelReleaseModel> for ModelRelease {
    fn from(model: ModelReleaseModel) -> Self {
        Self {
            id: model.id,
            lineage_key: model.lineage_key,
            provider_key: model.provider_key,
            tier: model.tier,
            release: model.release,
            created_at: model.created_at.into(),
        }
    }
}

/// Builds the release chain for template resolution.
///
/// Chain format uses optional `@provider` prefix:
/// `["@provider:lineage/tier:release", "@provider:lineage/tier", "lineage/tier:release", "lineage/tier", "lineage"]`
///
/// Template service uses this chain for fallback resolution:
/// 1. Provider-specific templates (for provider quirks)
/// 2. Shared templates (for lineage-wide prompting patterns)
/// 3. Base lineage templates (fallback)
pub fn build_release_chain(
    lineage_key: &str,
    tier: &str,
    release: Option<&str>,
    provider_key: &str,
) -> Vec<String> {
    let mut chain = Vec::with_capacity(5);

    // Provider-scoped templates (highest priority)
    if let Some(rel) = release {
        chain.push(format!(
            "@{}:{}/{}:{}",
            provider_key, lineage_key, tier, rel
        ));
    }
    chain.push(format!("@{}:{}/{}", provider_key, lineage_key, tier));

    // Provider-agnostic templates (shared across providers)
    if let Some(rel) = release {
        chain.push(format!("{}/{}:{}", lineage_key, tier, rel));
    }
    chain.push(format!("{}/{}", lineage_key, tier));

    // Lineage-wide templates (lowest priority)
    chain.push(lineage_key.to_string());

    chain
}

impl crate::traits::DataTransferObject for ModelRelease {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_release_chain_with_release() {
        let chain = build_release_chain("claude-3-5", "sonnet", Some("20241022"), "anthropic");
        assert_eq!(
            chain,
            vec![
                "@anthropic:claude-3-5/sonnet:20241022",
                "@anthropic:claude-3-5/sonnet",
                "claude-3-5/sonnet:20241022",
                "claude-3-5/sonnet",
                "claude-3-5",
            ]
        );
    }

    #[test]
    fn test_build_release_chain_without_release() {
        let chain = build_release_chain("vits", "ljspeech", None, "self-hosted");
        assert_eq!(
            chain,
            vec!["@self-hosted:vits/ljspeech", "vits/ljspeech", "vits",]
        );
    }

    #[test]
    fn test_build_release_chain_multi_provider() {
        // Same mistral model at self-hosted vs mistral API
        let self_hosted = build_release_chain("mistral", "7b", Some("v0.3"), "self-hosted");
        let mistral_api = build_release_chain("mistral", "7b", Some("v0.3"), "mistral");

        // Shared templates should match
        assert_eq!(self_hosted[2], mistral_api[2]); // "mistral/7b:v0.3"
        assert_eq!(self_hosted[3], mistral_api[3]); // "mistral/7b"
        assert_eq!(self_hosted[4], mistral_api[4]); // "mistral"

        // Provider-specific should differ
        assert_ne!(self_hosted[0], mistral_api[0]);
        assert_ne!(self_hosted[1], mistral_api[1]);
    }
}
