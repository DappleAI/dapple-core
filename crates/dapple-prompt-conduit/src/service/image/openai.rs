//! OpenAI images backend (direct HTTP, generation only).
//!
//! Talks to `/v1/images/generations`. Direct `reqwest` is used (not `rig`) so we
//! can set `output_format`/`background` on the gpt-image family — rig 0.28 drops
//! `additional_params` on its OpenAI image impl. There is no edit/inpaint path:
//! a model never reprocesses existing pixels (see `service/image/mod.rs`).

use super::{ImageBackend, ImageGenRequest};
use crate::error::ConduitError;
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct ImagesApiResponse {
    data: Vec<ImagesApiDatum>,
}

#[derive(Debug, Deserialize)]
struct ImagesApiDatum {
    b64_json: String,
}

/// Direct-HTTP OpenAI images backend.
pub struct OpenAiImageBackend {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl OpenAiImageBackend {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            // Trim a trailing slash so `{base}/images/...` never doubles up.
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
        }
    }

    /// True for the whole `gpt-image-*` family (gpt-image-1, gpt-image-2, …),
    /// which return base64 PNG and take `output_format`.
    fn is_gpt_image(model: &str) -> bool {
        model.starts_with("gpt-image")
    }

    /// Decode the first `b64_json` datum from an images API response body.
    fn decode_first(body: &str) -> Result<Vec<u8>, ConduitError> {
        let parsed: ImagesApiResponse = serde_json::from_str(body).map_err(|e| {
            ConduitError::ImageError(format!("failed to parse image response: {e}"))
        })?;
        let b64 = parsed
            .data
            .into_iter()
            .next()
            .ok_or_else(|| ConduitError::ImageError("image response had no data".to_string()))?
            .b64_json;
        STANDARD
            .decode(b64.trim())
            .map_err(|e| ConduitError::ImageError(format!("failed to base64-decode image: {e}")))
    }
}

#[async_trait]
impl ImageBackend for OpenAiImageBackend {
    async fn generate(&self, request: &ImageGenRequest) -> Result<Vec<u8>, ConduitError> {
        let mut body = json!({
            "model": request.model,
            "prompt": request.prompt,
            "size": request.size,
            "n": 1,
        });

        if Self::is_gpt_image(&request.model) {
            body["output_format"] = json!("png");
            // `quality` (low/medium/high/auto) sets the per-image detail budget.
            // Pass-through: when absent the provider uses its own default
            // (`auto`); `high` maximizes detail. Only sent for the gpt-image
            // family, whose vocabulary this matches (DALL·E uses standard/hd).
            if let Some(q) = &request.quality {
                body["quality"] = json!(q);
            }
            // `background` is an author/provider pass-through — NOT model-gated.
            // Transparent-background support is provider-specific, not a fixed
            // model property: OpenAI-direct gpt-image-2 rejects
            // `background:"transparent"`, but the fal.ai gpt-image-2 surface
            // supports it, and `base_url` is configurable. Send whatever the
            // author set in engine_body.background and let the provider validate,
            // rather than hardcoding a per-model capability table here.
            if let Some(bg) = &request.background {
                body["background"] = json!(bg);
            }
        } else {
            // DALL·E returns a URL unless asked for base64.
            body["response_format"] = json!("b64_json");
        }

        let resp = self
            .client
            .post(format!("{}/images/generations", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| ConduitError::ImageError(format!("image request failed: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| ConduitError::ImageError(format!("failed to read image response: {e}")))?;
        if !status.is_success() {
            return Err(ConduitError::ImageError(format!(
                "image provider error {status}: {text}"
            )));
        }
        Self::decode_first(&text)
    }

    /// Secondary backend: fires only when a prompt explicitly authors
    /// `provider = "openai"`. The default (no provider) now routes to the
    /// Fal/rath backend, which is the primary image path.
    fn supports(&self, request: &ImageGenRequest) -> bool {
        matches!(request.provider.as_deref(), Some("openai"))
    }

    fn name(&self) -> &str {
        "openai-image"
    }
}
