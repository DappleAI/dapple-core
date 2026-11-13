//! Fal image backend via rath-rs (generation only).
//!
//! Replaces the hand-rolled Stability SD3 backend. Images are reached through
//! rath's provider-agnostic `ImageClient` with Fal as the provider — e.g.
//! LayerDiffuse for native transparent-background pieces, flux for opaque
//! backdrops. The model slug and any Fal-model-specific knobs (transparency,
//! guidance, etc.) are authored in the guide's `:::inference-profile`
//! engine_body (`provider`/`model`/`provider_config` — see `handler/image.rs`);
//! this backend only builds the request and resolves the URL-or-base64 result.
//!
//! Generation-only: a model never reprocesses composed/uploaded pixels (see
//! `service/image/mod.rs`).

use super::{ImageBackend, ImageGenRequest};
use crate::error::ConduitError;
use crate::service::rath_util;
use async_trait::async_trait;
use rath::images::{ImageOptions, ImageRequest};
use serde_json::{json, Map, Value};

/// rath-backed Fal image backend.
pub struct RathImageBackend {
    http: reqwest::Client,
}

impl RathImageBackend {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
        }
    }

    /// Build from the environment. `None` unless `FAL_AUTH_KEY` is set — the same
    /// key rath resolves via `api_key_env`, so registering without it would only
    /// fail at generate time.
    pub fn from_env() -> Option<Self> {
        rath_util::fal_key_present().then(Self::new)
    }

    /// Assemble the Fal `provider_config`: conduit-derived defaults
    /// (`image_size` from the WxH size, `seed`) with the author's engine_body
    /// `provider_config` merged on top (author wins — they know the model's exact
    /// input schema: transparency flags, guidance scale, etc.).
    fn provider_config(request: &ImageGenRequest) -> Option<Value> {
        let mut cfg = Map::new();
        if let Some((w, h)) = parse_size(&request.size) {
            cfg.insert("image_size".to_string(), json!({ "width": w, "height": h }));
        }
        if let Some(seed) = request.seed.filter(|s| *s >= 0) {
            cfg.insert("seed".to_string(), Value::from(seed));
        }
        if let Some(Value::Object(author)) = &request.provider_config {
            for (k, v) in author {
                cfg.insert(k.clone(), v.clone());
            }
        }
        (!cfg.is_empty()).then_some(Value::Object(cfg))
    }
}

impl Default for RathImageBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a `WIDTHxHEIGHT` size into `(w, h)`, or `None` when it can't be parsed.
fn parse_size(size: &str) -> Option<(u32, u32)> {
    let mut parts = size.split(['x', 'X']);
    let w = parts.next()?.trim().parse::<u32>().ok()?;
    let h = parts.next()?.trim().parse::<u32>().ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

#[async_trait]
impl ImageBackend for RathImageBackend {
    async fn generate(&self, request: &ImageGenRequest) -> Result<Vec<u8>, ConduitError> {
        let url = rath_util::fal_model_url(&request.model);
        // Leave `ImageRequest.size` None: rath stringifies it into Fal's
        // `image_size`, but we set that as a {width,height} object via
        // provider_config instead so any WxH size parses cleanly.
        let req = ImageRequest {
            prompt: request.prompt.clone(),
            model: None,
            size: None,
            provider_config: Self::provider_config(request),
        };
        let client = ImageOptions::default()
            .create(&url)
            .map_err(rath_util::image_err)?;
        let response = client
            .generate_image(&req)
            .await
            .map_err(rath_util::image_err)?;
        let first = response.images.first().ok_or_else(|| {
            ConduitError::ImageError("fal image response had no images".to_string())
        })?;
        rath_util::image_bytes(&self.http, first).await
    }

    /// Routes the Fal provider — and the default (no provider), now that Fal is
    /// the primary image path. `openai` is claimed by the secondary OpenAI
    /// backend; the stub is the unconditional fallback.
    fn supports(&self, request: &ImageGenRequest) -> bool {
        matches!(
            request.provider.as_deref(),
            None | Some("") | Some("fal") | Some("rath")
        )
    }

    fn name(&self) -> &str {
        "fal-image"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(provider: Option<&str>) -> ImageGenRequest {
        ImageGenRequest {
            provider: provider.map(String::from),
            prompt: "a teeming market square".to_string(),
            model: "fal-ai/flux/schnell".to_string(),
            size: "1536x1024".to_string(),
            background: None,
            quality: None,
            seed: None,
            negative_prompt: None,
            provider_config: None,
        }
    }

    #[test]
    fn supports_fal_and_default_not_openai() {
        let b = RathImageBackend::new();
        assert!(b.supports(&req(None)));
        assert!(b.supports(&req(Some(""))));
        assert!(b.supports(&req(Some("fal"))));
        assert!(b.supports(&req(Some("rath"))));
        assert!(!b.supports(&req(Some("openai"))));
    }

    #[test]
    fn parse_size_parses_and_guards() {
        assert_eq!(parse_size("1536x1024"), Some((1536, 1024)));
        assert_eq!(parse_size("1024X1024"), Some((1024, 1024)));
        assert_eq!(parse_size("garbage"), None);
        assert_eq!(parse_size("0x100"), None);
    }

    #[test]
    fn provider_config_maps_size_and_seed() {
        let mut r = req(Some("fal"));
        r.seed = Some(42);
        let cfg = RathImageBackend::provider_config(&r).unwrap();
        assert_eq!(cfg["image_size"], json!({ "width": 1536, "height": 1024 }));
        assert_eq!(cfg["seed"], json!(42));
    }

    #[test]
    fn author_provider_config_overrides_defaults() {
        let mut r = req(Some("fal"));
        r.seed = Some(1);
        // Author sets a transparency flag and overrides image_size + seed.
        r.provider_config = Some(json!({
            "enable_transparency": true,
            "seed": 99,
            "image_size": "square_hd"
        }));
        let cfg = RathImageBackend::provider_config(&r).unwrap();
        assert_eq!(cfg["enable_transparency"], json!(true));
        assert_eq!(cfg["seed"], json!(99), "author seed wins");
        assert_eq!(
            cfg["image_size"],
            json!("square_hd"),
            "author image_size wins"
        );
    }

    #[test]
    fn provider_config_empty_when_nothing_to_send() {
        let mut r = req(Some("fal"));
        r.size = "garbage".to_string();
        r.seed = None;
        r.provider_config = None;
        assert!(RathImageBackend::provider_config(&r).is_none());
    }
}
