//! Thin HTTP client for Conduit's `/embed` endpoint.
//!
//! The API owns the full embedding pipeline (block extraction, metadata
//! construction, DB writes) while Conduit handles only the compute-intensive
//! embedding generation via ONNX.
//!
//! One call carries ONE BATCH, not one guide: the caller
//! (`handler::guide_embeddings`) splits a guide's blocks into
//! `EMBED_BATCH_SIZE` chunks, because ModernBERT pads a batch to its longest
//! member and a whole-guide call made peak memory scale with the guide's size
//! (it OOM-killed conduit-onnx). The 120s timeout below is therefore a
//! per-batch budget, not a per-guide one.

use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct EmbeddingClient {
    client: Client,
    base_url: String,
}

#[derive(Serialize)]
struct EmbedRequest {
    texts: Vec<String>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
    #[allow(dead_code)]
    dimension: usize,
    /// `#[serde(default)]` so this client works against a conduit that predates
    /// the field — no lockstep deploy required; an older conduit simply reports
    /// no truncation.
    #[serde(default)]
    truncated_count: usize,
}

/// The result of one `/embed` call.
pub struct EmbedBatch {
    /// One embedding per input text, in order.
    pub embeddings: Vec<Vec<f32>>,
    /// How many of the submitted texts the backend TRUNCATED to fit its
    /// sequence cap.
    ///
    /// This is the drift alarm for the char-based window size in
    /// [`crate::service::text_window`]. That module sizes windows against the
    /// conduit's token cap using a chars-per-token *estimate*, which dense
    /// markup or CJK can beat. Non-zero here means the estimate lost and the
    /// vectors do not cover the whole text — so an ingest must fail rather than
    /// commit an index that silently omits content.
    pub truncated_count: usize,
}

impl EmbeddingClient {
    pub fn new(conduit_url: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: conduit_url.trim_end_matches('/').to_string(),
        }
    }

    /// Generate embeddings for a batch of texts.
    ///
    /// Returns `Ok(None)` if the service is unavailable (graceful degradation
    /// when Conduit has no embedding service configured). Returns
    /// `Ok(Some(embeddings))` on success, or `Err` on unexpected failures.
    pub async fn embed(&self, texts: &[String]) -> Result<Option<EmbedBatch>, String> {
        if texts.is_empty() {
            return Ok(Some(EmbedBatch {
                embeddings: Vec::new(),
                truncated_count: 0,
            }));
        }

        let url = format!("{}/embed", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(&EmbedRequest {
                texts: texts.to_vec(),
            })
            .timeout(std::time::Duration::from_secs(120)) // ONNX inference can be slow
            .send()
            .await
            .map_err(|e| format!("Conduit embed request failed: {}", e))?;

        if response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
            // Conduit has no embedding service -- degrade gracefully
            return Ok(None);
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Conduit embed returned {}: {}", status, body));
        }

        let result: EmbedResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse embed response: {}", e))?;

        Ok(Some(EmbedBatch {
            embeddings: result.embeddings,
            truncated_count: result.truncated_count,
        }))
    }
}
