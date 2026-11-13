//! HTTP client for conduit-onnx sub-service.
//!
//! Proxies embedding, summarization, and MLM requests to the
//! internal conduit-onnx service via HTTP.

use crate::error::ConduitError;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Client for communicating with the conduit-onnx internal service.
pub struct OnnxInferenceClient {
    client: Client,
    base_url: String,
    internal_key: Option<String>,
}

// -- Embedding types --

#[derive(Debug, Serialize)]
struct EmbedRequest {
    texts: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
    dimension: usize,
    /// `#[serde(default)]` so this client keeps working against a conduit-onnx
    /// that predates the field — an older pod simply reports no truncation,
    /// which is the same answer it could give before.
    #[serde(default)]
    truncated_count: usize,
}

/// The result of one logical `/embed` call, however many HTTP round-trips it
/// took internally.
#[derive(Debug, Clone)]
pub struct OnnxEmbedBatch {
    pub embeddings: Vec<Vec<f32>>,
    pub dimension: usize,
    /// Texts conduit-onnx TRUNCATED to fit its sequence cap. Non-zero means
    /// those vectors do not represent the whole text, which is fatal for a
    /// caller that is about to persist them as a search index.
    pub truncated_count: usize,
}

// -- Summarization types --

#[derive(Debug, Serialize)]
pub struct SummarizeRequest {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_prefix: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SummarizeResponse {
    pub summary: String,
    pub input_length: usize,
    pub output_length: usize,
}

// -- MLM types --

#[derive(Debug, Serialize)]
pub struct MadlibRequest {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<usize>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TokenPrediction {
    pub token: String,
    pub score: f32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MaskPrediction {
    pub position: usize,
    pub candidates: Vec<TokenPrediction>,
}

#[derive(Debug, Deserialize)]
pub struct MadlibResponse {
    pub filled_text: String,
    pub predictions: Vec<MaskPrediction>,
}

impl OnnxInferenceClient {
    pub fn new(base_url: &str, internal_key: Option<String>) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            internal_key,
        }
    }

    /// Maximum texts per HTTP request to conduit-onnx.
    /// Keeps peak memory bounded on the inference pod.
    const MAX_BATCH_SIZE: usize = 8;

    /// Generate embeddings for a batch of texts.
    /// Returns None if the service is unavailable (503).
    /// Large batches are split into chunks of MAX_BATCH_SIZE to avoid OOM.
    pub async fn embed(&self, texts: &[String]) -> Result<Option<OnnxEmbedBatch>, ConduitError> {
        let url = format!("{}/embed", self.base_url);
        let mut all_embeddings = Vec::with_capacity(texts.len());
        let mut dimension = 0usize;
        // Summed across chunks so the count describes the caller's whole
        // request, not whichever chunk happened to contain the long text.
        let mut truncated_count = 0usize;

        for chunk in texts.chunks(Self::MAX_BATCH_SIZE) {
            let req = EmbedRequest {
                texts: chunk.to_vec(),
            };

            let mut request = self.client.post(&url).json(&req);
            if let Some(ref key) = self.internal_key {
                request = request.header("x-internal-key", key);
            }

            let response = request.send().await.map_err(|e| {
                ConduitError::EmbeddingError(format!("conduit-onnx request failed: {}", e))
            })?;

            if response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                return Ok(None);
            }

            if !response.status().is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(ConduitError::EmbeddingError(format!(
                    "conduit-onnx /embed failed: {}",
                    body
                )));
            }

            let resp: EmbedResponse = response.json().await.map_err(|e| {
                ConduitError::EmbeddingError(format!("Failed to parse embed response: {}", e))
            })?;

            dimension = resp.dimension;
            truncated_count += resp.truncated_count;
            all_embeddings.extend(resp.embeddings);
        }

        Ok(Some(OnnxEmbedBatch {
            embeddings: all_embeddings,
            dimension,
            truncated_count,
        }))
    }

    /// Generate a single embedding vector for a text.
    ///
    /// Deliberately ignores `truncated_count`: the callers here are RAG and
    /// overlay QUERIES, which clamp their input on purpose and only need a
    /// representative vector. Truncation matters where vectors are persisted as
    /// an index — see [`OnnxInferenceClient::embed`].
    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>, ConduitError> {
        let result = self.embed(&[text.to_string()]).await?.ok_or_else(|| {
            ConduitError::EmbeddingError("Embedding service unavailable".to_string())
        })?;

        result
            .embeddings
            .into_iter()
            .next()
            .ok_or_else(|| ConduitError::EmbeddingError("Empty embedding response".to_string()))
    }

    /// Summarize text.
    pub async fn summarize(
        &self,
        req: SummarizeRequest,
    ) -> Result<SummarizeResponse, ConduitError> {
        let url = format!("{}/summarize", self.base_url);

        let mut request = self.client.post(&url).json(&req);
        if let Some(ref key) = self.internal_key {
            request = request.header("x-internal-key", key);
        }

        let response = request.send().await.map_err(|e| {
            ConduitError::SummarizationError(format!("conduit-onnx request failed: {}", e))
        })?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ConduitError::SummarizationError(format!(
                "conduit-onnx /summarize failed: {}",
                body
            )));
        }

        response.json().await.map_err(|e| {
            ConduitError::SummarizationError(format!("Failed to parse response: {}", e))
        })
    }

    /// Fill mask tokens in text.
    pub async fn madlib(&self, req: MadlibRequest) -> Result<MadlibResponse, ConduitError> {
        let url = format!("{}/madlib", self.base_url);

        let mut request = self.client.post(&url).json(&req);
        if let Some(ref key) = self.internal_key {
            request = request.header("x-internal-key", key);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ConduitError::MlmError(format!("conduit-onnx request failed: {}", e)))?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ConduitError::MlmError(format!(
                "conduit-onnx /madlib failed: {}",
                body
            )));
        }

        response
            .json()
            .await
            .map_err(|e| ConduitError::MlmError(format!("Failed to parse response: {}", e)))
    }
}
