//! HTTP client for communicating with the conduit service

use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Errors from the conduit client
#[derive(Debug, Error)]
pub enum ConduitClientError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Failed to parse SSE event: {0}")]
    SseParse(String),

    #[error("Prompt execution error: {code} - {message}")]
    PromptError {
        code: String,
        message: String,
        retryable: bool,
    },

    #[error("Summarize error: {code} - {message}")]
    SummarizeError { code: String, message: String },

    #[error("Madlib error: {code} - {message}")]
    MadlibError { code: String, message: String },

    #[error("Search error: {code} - {message}")]
    SearchError { code: String, message: String },

    #[error("Image composite error: {code} - {message}")]
    ImageCompositeError { code: String, message: String },

    #[error("Audio analysis error: {code} - {message}")]
    AudioAnalysisError { code: String, message: String },

    #[error("A/V compose error: {code} - {message}")]
    ComposeAvError { code: String, message: String },

    #[error("Document render error: {code} - {message}")]
    DocumentRenderError { code: String, message: String },

    #[error("Failed to parse response: {0}")]
    ParseError(String),

    #[error("Unexpected error: {0}")]
    Unexpected(String),
}

/// Request to execute a prompt via conduit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptRequest {
    pub prompt_id: Uuid,
    pub attempt_id: Uuid,
    pub playbook_task_id: Uuid,
    /// Optional context data for the prompt (e.g., text, max_length, top_k)
    /// Used by summarize, madlib, synth commands to pass input and parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_data: Option<serde_json::Value>,
    /// RAG context for guide content retrieval.
    /// When present with non-empty guide_ids, Conduit will query pgvector
    /// and inject retrieved chunks as system context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag_context: Option<serde_json::Value>,
    /// Dispatch ID for Phase F runtime task-context injection. Paired with
    /// `task_id` it lets Conduit fetch runtime dispatch state from api and
    /// prepend it as a system message. Absent for non-dispatch callers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_id: Option<Uuid>,
    /// Dispatch-task ID for Phase F runtime task-context injection. This is
    /// the `dispatch_tasks.id` (i.e. the instance), not the source task_id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
}

/// SSE events from conduit prompt execution
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptEvent {
    Started {
        request_id: Uuid,
        model: String,
        prompt_id: Uuid,
    },
    Token {
        content: String,
    },
    Done {
        output_tokens: u32,
        input_tokens: u32,
        latency_ms: u64,
        finish_reason: String,
    },
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
    /// An asset was created (for TTS and other binary outputs)
    Asset {
        asset_id: Uuid,
        asset_kind: String,
        mime_type: String,
        content_length: i64,
        duration_seconds: f32,
        sample_rate: u32,
        text: String,
    },
}

/// Token usage data extracted from SSE Done events
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub latency_ms: u64,
}

/// Result from prompt execution - can be text tokens or an asset
#[derive(Debug, Clone)]
pub enum PromptResult {
    /// Text result from LLM (concatenated tokens) with usage metadata
    Text(TextResult),
    /// Asset result from TTS or similar binary output
    Asset(AssetResult),
}

/// Text result with content and token usage
#[derive(Debug, Clone)]
pub struct TextResult {
    pub content: String,
    pub usage: TokenUsage,
}

/// Asset information from TTS or similar binary output
#[derive(Debug, Clone)]
pub struct AssetResult {
    pub asset_id: Uuid,
    pub asset_kind: String,
    pub mime_type: String,
    pub content_length: i64,
    pub duration_seconds: f32,
    pub sample_rate: u32,
    pub text: String,
}

/// Request to summarize text via conduit
#[derive(Debug, Serialize)]
pub struct SummarizeRequest {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_length: Option<usize>,
    /// Direct prompt prefix for T5 (e.g., "summarize:", "compress:")
    /// Resolved from prompt_data metadata in flow commands
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_prefix: Option<String>,
}

/// Response from summarization
#[derive(Debug, Deserialize)]
pub struct SummarizeResponse {
    pub summary: String,
    pub input_length: usize,
    pub output_length: usize,
}

/// Request to fill mask tokens via conduit
#[derive(Debug, Serialize)]
pub struct MadlibRequest {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<usize>,
}

/// A single token prediction with score
#[derive(Debug, Deserialize, Clone)]
pub struct TokenPrediction {
    pub token: String,
    pub score: f32,
}

/// Prediction for a single mask position
#[derive(Debug, Deserialize, Clone)]
pub struct MaskPrediction {
    pub position: usize,
    pub candidates: Vec<TokenPrediction>,
}

/// Response from mask filling
#[derive(Debug, Deserialize)]
pub struct MadlibResponse {
    pub filled_text: String,
    pub predictions: Vec<MaskPrediction>,
}

/// Request to search guide content via conduit
#[derive(Debug, Serialize)]
pub struct SearchRequest {
    pub q: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub locale: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub guide_ids: Vec<Uuid>,
}

/// Response from guide content search
#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResultItem>,
    pub count: usize,
}

/// A single search result item
#[derive(Debug, Deserialize, Clone)]
pub struct SearchResultItem {
    pub block_id: Uuid,
    pub guide_id: Uuid,
    pub chapter_id: Uuid,
    pub directive_type: String,
    pub chunk_text: String,
    pub metadata: serde_json::Value,
    pub similarity: f64,
}

/// One block placement for a composite (`{asset_id,x,y,w,h}`).
#[derive(Debug, Clone, Serialize)]
pub struct CompositeBlock {
    pub asset_id: Uuid,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Request to deterministically composite generated pieces (`POST /composite`).
#[derive(Debug, Clone, Serialize)]
pub struct ImageCompositeRequest {
    pub background_asset_id: Uuid,
    pub blocks: Vec<CompositeBlock>,
    /// Reinhard per-channel color match of each block to the background.
    pub color_match: bool,
    /// Border feather radius in pixels (alpha ramp); 0 disables.
    pub feather: u32,
    /// Drop a soft procedural contact shadow under each block.
    pub shadow: bool,
    /// Optional output canvas width; with height, upscales the background before compositing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canvas_width: Option<u32>,
    /// Optional output canvas height (see `canvas_width`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canvas_height: Option<u32>,
}

/// Response from a composite.
#[derive(Debug, Deserialize, Clone)]
pub struct ImageCompositeResponse {
    pub asset_id: Uuid,
    pub asset_kind: String,
    pub mime_type: String,
    pub content_length: i64,
    pub width: u32,
    pub height: u32,
}

/// One piece for a strategy-driven compose (`{asset_id, shape?, weight?}`). The
/// caller does NOT position it — the conduit measures + lays it out. `shape`/
/// `weight` are forward-compat hints (the `panorama` strategy reads aspect from
/// the pixels and ignores them).
#[derive(Debug, Clone, Serialize)]
pub struct ComposePiece {
    pub asset_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f32>,
    /// Human layer name for the SVG output (e.g. `"Scene 1"`). Names the piece's
    /// `<g>` layer in SVG mode; ignored by the raster path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Layout knobs for a compose. Every field is optional: an unset key falls back
/// to the conduit's `LayoutParams` default, so the request carries only overrides.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ComposeParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub near_height_frac: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub far_height_frac: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side_margin_frac: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub far_side_margin_frac: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bands: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stagger_frac: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_jitter_frac: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlap_frac: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_clear_top_frac: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_clear_bottom_frac: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// Request to assemble pieces with a server-side layout strategy (`POST /compose`).
/// Shares [`ImageCompositeResponse`] with `/composite`.
#[derive(Debug, Clone, Serialize)]
pub struct ImageComposeRequest {
    pub background_asset_id: Uuid,
    pub pieces: Vec<ComposePiece>,
    pub strategy: String,
    pub params: ComposeParams,
    pub color_match: bool,
    pub feather: u32,
    pub shadow: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canvas_width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canvas_height: Option<u32>,
    /// Output serialization: `"svg"` for a self-contained, hand-editable layout
    /// document (named movable layers); omitted/`"png"` flattens to a raster.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_format: Option<String>,
    /// Optional silhouette SVG asset: clip the composed spread to the mark
    /// (the video-quilt reveal's overlay trick).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silhouette_asset_id: Option<Uuid>,
}

/// Request for `POST /render-document` — arbitrary markdown rendered to a PDF
/// plus a self-contained HTML page, both persisted as `document` assets.
/// Deterministic and local on the conduit (dapple-render / Typst, embedded
/// fonts); no model. The PDF carries internal anchor links, an outline, and
/// an optional TOC; the HTML mirrors the same anchors and theme.
#[derive(Debug, Clone, Serialize)]
pub struct RenderDocumentRequest {
    pub title: String,
    pub markdown: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Preformatted date display string (rendered verbatim on the byline).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Branding style key: `consumer` | `technical` | `internal`.
    pub branding: String,
    /// Table-of-contents override: `Some(true)` forces a TOC page,
    /// `Some(false)` suppresses it, `None` = automatic (≥9 h1–h3 headings).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toc: Option<bool>,
}

/// Response from `POST /render-document`.
#[derive(Debug, Clone, Deserialize)]
pub struct RenderDocumentResponse {
    /// The PDF asset (`application/pdf`).
    pub asset_id: Uuid,
    pub asset_kind: String,
    pub mime_type: String,
    pub content_length: i64,
    /// The HTML asset (`text/html`) — same document as a self-contained page
    /// for the inline `display: document` viewer.
    pub html_asset_id: Uuid,
    pub html_content_length: i64,
}

// ============================================================================
// A/V compose (`POST /compose-av`) — the audio+video analog of image `/compose`.
// Assembles generated audio/video piece assets into an editable OTIO timeline
// document. No pixels/samples are read; clips reference pieces by asset URL.
// ============================================================================

/// One generated audio/video piece for `POST /compose-av`. `duration_seconds` is
/// required (the producer knows it; v1 does not probe media containers). `kind`
/// (`video`/`audio`) is optional — the conduit derives it from the asset's kind.
#[derive(Debug, Clone, Serialize)]
pub struct ComposeAvPiece {
    pub asset_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub duration_seconds: f64,
    /// Mix path (`output: "wav"`) only: how this clip relates to a prior clip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relate: Option<ComposeAvRelate>,
    /// Mix path only: linear playback gain (< 1 = the mutter-down).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain: Option<f64>,
}

/// A clip's relation to a prior clip on the conversation timeline (mix path only).
#[derive(Debug, Clone, Serialize)]
pub struct ComposeAvRelate {
    /// Index of the anchor piece (must be earlier in the list).
    pub to: usize,
    /// `consecutive` (follows) or `interrupting` (overlaps); unset → consecutive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Seconds; interpreted per `mode`; unset → 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset_seconds: Option<f64>,
}

/// Arrangement knobs for a compose-av. Every field optional (unset → conduit
/// `TimelineParams` default), so the request carries only overrides.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ComposeAvParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_frames: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// Request to assemble A/V pieces into a timeline document (`POST /compose-av`).
#[derive(Debug, Clone, Serialize)]
pub struct ComposeAvRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps_num: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps_den: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_base: Option<String>,
    pub strategy: String,
    pub params: ComposeAvParams,
    pub format: String,
    /// Optional song-map asset (from `audio analyze`); its shifts become
    /// timeline markers. Omitted from the wire when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song_map_asset_id: Option<Uuid>,
    /// Deliverable: unset/`document` → OTIO timeline; `wav` → mixed conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// `strategy: "document"` only: the guide-compiled composition document
    /// (version 3), passed through verbatim. The conduit validates it and
    /// persists it for the renderer (`format: "render"`) or exports it as an
    /// editable timeline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<serde_json::Value>,
    pub pieces: Vec<ComposeAvPiece>,
}

/// Response from `POST /compose-av`.
#[derive(Debug, Deserialize, Clone)]
pub struct ComposeAvResponse {
    pub asset_id: Uuid,
    pub asset_kind: String,
    pub mime_type: String,
    pub content_length: i64,
    pub format: String,
    pub clip_count: usize,
}

/// Client for communicating with the conduit service
pub struct ConduitClient {
    client: Client,
    base_url: String,
    /// Shared secret sent as the `X-Internal-Key` header to conduit-oximedia,
    /// which enforces it on every non-health endpoint whenever its own
    /// `CONDUIT_INTERNAL_KEY` is set (the secret is created on essentially every
    /// deploy). Read once from the environment at construction; `None` (or an
    /// empty var) omits the header — matching oximedia's "unset key ⇒ no
    /// enforcement" behavior. The main conduit ignores this header, so it is
    /// only attached on the oximedia-bound methods (`compose_av`,
    /// `analyze_audio`) and never on `/prompt`, `/embed`, `/compose`, etc.
    internal_key: Option<String>,
}

impl ConduitClient {
    /// Create a new conduit client
    pub fn new(base_url: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            internal_key: std::env::var("CONDUIT_INTERNAL_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }

    /// Attach the `X-Internal-Key` auth header when a key was configured.
    /// Applied only on the conduit-oximedia-bound requests; the main conduit
    /// ignores the header so it is deliberately not added to `/prompt` & friends.
    fn with_internal_key(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.internal_key {
            Some(key) => rb.header("X-Internal-Key", key),
            None => rb,
        }
    }

    /// Execute a prompt and collect the full response
    /// Blocks until streaming completes and returns either text tokens or an asset
    pub async fn execute_prompt(
        &self,
        request: PromptRequest,
    ) -> Result<PromptResult, ConduitClientError> {
        let url = format!("{}/prompt", self.base_url);

        log::debug!(
            "Executing prompt: url={}, prompt_id={}, attempt_id={}, playbook_task_id={}",
            url,
            request.prompt_id,
            request.attempt_id,
            request.playbook_task_id
        );

        // Make the POST request expecting SSE response
        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ConduitClientError::Unexpected(format!(
                "HTTP {} - {}",
                status, body
            )));
        }

        // Stream the SSE response and collect tokens or asset
        let mut collected_tokens: Vec<String> = Vec::new();
        let mut asset_result: Option<AssetResult> = None;
        let mut token_usage = TokenUsage::default();
        let mut stream = response.bytes_stream();

        // Buffer for incomplete SSE lines
        let mut buffer = String::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            let chunk_str = String::from_utf8_lossy(&chunk);
            buffer.push_str(&chunk_str);

            // Process complete SSE lines
            while let Some(line_end) = buffer.find("\n\n") {
                let event_block = buffer[..line_end].to_string();
                buffer = buffer[line_end + 2..].to_string();

                // Parse SSE event
                if let Some(event) = parse_sse_event(&event_block)? {
                    match event {
                        PromptEvent::Started { request_id, .. } => {
                            log::debug!("Prompt execution started: request_id={}", request_id);
                        }
                        PromptEvent::Token { content } => {
                            collected_tokens.push(content);
                        }
                        PromptEvent::Asset {
                            asset_id,
                            asset_kind,
                            mime_type,
                            content_length,
                            duration_seconds,
                            sample_rate,
                            text,
                        } => {
                            log::debug!(
                                "Asset created: asset_id={}, duration={}s",
                                asset_id,
                                duration_seconds
                            );
                            asset_result = Some(AssetResult {
                                asset_id,
                                asset_kind,
                                mime_type,
                                content_length,
                                duration_seconds,
                                sample_rate,
                                text,
                            });
                        }
                        PromptEvent::Done {
                            output_tokens,
                            input_tokens,
                            latency_ms,
                            finish_reason,
                        } => {
                            log::debug!(
                                "Prompt execution done: output_tokens={}, input_tokens={}, latency_ms={}, finish_reason={}",
                                output_tokens,
                                input_tokens,
                                latency_ms,
                                finish_reason
                            );
                            token_usage = TokenUsage {
                                input_tokens,
                                output_tokens,
                                latency_ms,
                            };
                        }
                        PromptEvent::Error {
                            code,
                            message,
                            retryable,
                        } => {
                            return Err(ConduitClientError::PromptError {
                                code,
                                message,
                                retryable,
                            });
                        }
                    }
                }
            }
        }

        // Return asset if we received one, otherwise return collected text tokens
        if let Some(asset) = asset_result {
            Ok(PromptResult::Asset(asset))
        } else {
            Ok(PromptResult::Text(TextResult {
                content: collected_tokens.join(""),
                usage: token_usage,
            }))
        }
    }

    /// Summarize text via the conduit service
    pub async fn summarize(
        &self,
        request: SummarizeRequest,
    ) -> Result<SummarizeResponse, ConduitClientError> {
        let url = format!("{}/summarize", self.base_url);

        log::debug!(
            "Summarizing text: url={}, text_len={}, max_length={:?}, min_length={:?}, prompt_prefix={:?}",
            url,
            request.text.len(),
            request.max_length,
            request.min_length,
            request.prompt_prefix
        );

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            response
                .json()
                .await
                .map_err(|e| ConduitClientError::ParseError(e.to_string()))
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(ConduitClientError::SummarizeError {
                code: status.to_string(),
                message: body,
            })
        }
    }

    /// Fill mask tokens via the conduit service
    pub async fn madlib(
        &self,
        request: MadlibRequest,
    ) -> Result<MadlibResponse, ConduitClientError> {
        let url = format!("{}/madlib", self.base_url);

        log::debug!(
            "Filling masks: url={}, text_len={}, top_k={:?}",
            url,
            request.text.len(),
            request.top_k
        );

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            response
                .json()
                .await
                .map_err(|e| ConduitClientError::ParseError(e.to_string()))
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(ConduitClientError::MadlibError {
                code: status.to_string(),
                message: body,
            })
        }
    }

    /// Search guide content via the conduit service
    pub async fn search(
        &self,
        request: SearchRequest,
    ) -> Result<SearchResponse, ConduitClientError> {
        let url = format!("{}/search", self.base_url);

        log::debug!(
            "Searching guide content: url={}, q={}, limit={:?}",
            url,
            request.q,
            request.limit
        );

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            response
                .json()
                .await
                .map_err(|e| ConduitClientError::ParseError(e.to_string()))
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(ConduitClientError::SearchError {
                code: status.to_string(),
                message: body,
            })
        }
    }

    /// Deterministically composite generated pieces via the conduit service
    /// (`POST /composite`). No model is involved — purely local pixel assembly.
    pub async fn composite_images(
        &self,
        request: ImageCompositeRequest,
    ) -> Result<ImageCompositeResponse, ConduitClientError> {
        let url = format!("{}/composite", self.base_url);

        log::debug!(
            "Compositing image: url={}, background_asset_id={}, blocks={}",
            url,
            request.background_asset_id,
            request.blocks.len()
        );

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            response
                .json()
                .await
                .map_err(|e| ConduitClientError::ParseError(e.to_string()))
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(ConduitClientError::ImageCompositeError {
                code: status.to_string(),
                message: body,
            })
        }
    }

    /// Assemble generated pieces with a server-side layout strategy
    /// (`POST /compose`). The conduit reads each piece's pixels, trims it to its
    /// content, lays them out deterministically, and composites. No model is
    /// involved — purely local pixel assembly.
    pub async fn compose_images(
        &self,
        request: ImageComposeRequest,
    ) -> Result<ImageCompositeResponse, ConduitClientError> {
        let url = format!("{}/compose", self.base_url);

        log::debug!(
            "Composing image: url={}, background_asset_id={}, pieces={}, strategy={}",
            url,
            request.background_asset_id,
            request.pieces.len(),
            request.strategy
        );

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            response
                .json()
                .await
                .map_err(|e| ConduitClientError::ParseError(e.to_string()))
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(ConduitClientError::ImageCompositeError {
                code: status.to_string(),
                message: body,
            })
        }
    }

    /// Render a markdown document to a PDF asset via the conduit
    /// `POST /render-document` endpoint. Main-conduit endpoint — no
    /// `X-Internal-Key` involved (that header is oximedia-only).
    pub async fn render_document(
        &self,
        request: RenderDocumentRequest,
    ) -> Result<RenderDocumentResponse, ConduitClientError> {
        let url = format!("{}/render-document", self.base_url);

        log::debug!(
            "Rendering document: url={}, title={:?}, markdown_chars={}, branding={}",
            url,
            request.title,
            request.markdown.chars().count(),
            request.branding
        );

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            response
                .json()
                .await
                .map_err(|e| ConduitClientError::ParseError(e.to_string()))
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(ConduitClientError::DocumentRenderError {
                code: status.to_string(),
                message: body,
            })
        }
    }

    /// Assemble generated A/V pieces into an editable OTIO timeline document via
    /// the conduit `POST /compose-av` endpoint.
    pub async fn compose_av(
        &self,
        request: ComposeAvRequest,
    ) -> Result<ComposeAvResponse, ConduitClientError> {
        let url = format!("{}/compose-av", self.base_url);

        log::debug!(
            "Composing A/V timeline: url={}, pieces={}, strategy={}, format={}",
            url,
            request.pieces.len(),
            request.strategy,
            request.format
        );

        let response = self
            .with_internal_key(
                self.client
                    .post(&url)
                    .header("Content-Type", "application/json"),
            )
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            response
                .json()
                .await
                .map_err(|e| ConduitClientError::ParseError(e.to_string()))
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(ConduitClientError::ComposeAvError {
                code: status.to_string(),
                message: body,
            })
        }
    }
}

/// Parse an SSE event block into a PromptEvent
fn parse_sse_event(event_block: &str) -> Result<Option<PromptEvent>, ConduitClientError> {
    let mut data_line: Option<&str> = None;

    for line in event_block.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            data_line = Some(data.trim());
        }
    }

    if let Some(data) = data_line {
        if data.is_empty() {
            return Ok(None);
        }

        let event: PromptEvent = serde_json::from_str(data).map_err(|e| {
            ConduitClientError::SseParse(format!("Failed to parse: {} - {}", data, e))
        })?;

        Ok(Some(event))
    } else {
        Ok(None)
    }
}

/// One stem to analyze (audio-analysis request item).
#[derive(Debug, Clone, Serialize)]
pub struct AnalyzeAudioStem {
    pub asset_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// Request body for the conduit-oximedia `POST /analyze-audio` endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct AnalyzeAudioRequest {
    pub stems: Vec<AnalyzeAudioStem>,
    /// Optional mastered full mix — section-detection input, never a stem.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mix: Option<AnalyzeAudioStem>,
}

/// At-a-glance song-map summary returned alongside the persisted asset.
#[derive(Debug, Clone, Deserialize)]
pub struct AnalyzeAudioSummary {
    pub duration_seconds: f64,
    pub stem_count: usize,
    pub sample_rate: u32,
    pub shift_count: usize,
    pub tempo_bpm: Option<f32>,
    /// Number of detected mix sections (0 when no mix was provided).
    #[serde(default)]
    pub section_count: usize,
    /// Aggregate section-structure confidence (duration-weighted mean).
    #[serde(default)]
    pub structure_confidence: Option<f64>,
    /// Per-stem class + hit count, in stem order.
    #[serde(default)]
    pub stems: Vec<AnalyzeAudioStemSummary>,
}

/// Per-stem entry of [`AnalyzeAudioSummary`].
#[derive(Debug, Clone, Deserialize)]
pub struct AnalyzeAudioStemSummary {
    pub name: String,
    pub class: String,
    pub hit_count: usize,
}

/// Response body for `POST /analyze-audio`.
#[derive(Debug, Clone, Deserialize)]
pub struct AnalyzeAudioResponse {
    pub asset_id: Uuid,
    pub asset_kind: String,
    pub mime_type: String,
    pub content_length: i64,
    pub summary: AnalyzeAudioSummary,
}

impl ConduitClient {
    /// Analyze multitrack WAV stems into a song-map asset via the
    /// conduit-oximedia `POST /analyze-audio` endpoint.
    pub async fn analyze_audio(
        &self,
        request: AnalyzeAudioRequest,
    ) -> Result<AnalyzeAudioResponse, ConduitClientError> {
        let url = format!("{}/analyze-audio", self.base_url);

        log::debug!(
            "Analyzing audio: url={}, stems={}",
            url,
            request.stems.len()
        );

        let response = self
            .with_internal_key(
                self.client
                    .post(&url)
                    .header("Content-Type", "application/json"),
            )
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            response
                .json()
                .await
                .map_err(|e| ConduitClientError::ParseError(e.to_string()))
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(ConduitClientError::AudioAnalysisError {
                code: status.to_string(),
                message: body,
            })
        }
    }
}

impl Default for ConduitClient {
    fn default() -> Self {
        Self::new("http://localhost:8081")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_event_token() {
        let event_block = "data:{\"type\":\"token\",\"content\":\"Hello\"}";
        let result = parse_sse_event(event_block).unwrap();
        assert!(matches!(result, Some(PromptEvent::Token { content }) if content == "Hello"));
    }

    #[test]
    fn test_parse_sse_event_started() {
        let event_block = "data:{\"type\":\"started\",\"request_id\":\"550e8400-e29b-41d4-a716-446655440000\",\"model\":\"claude-3-sonnet\",\"prompt_id\":\"550e8400-e29b-41d4-a716-446655440001\"}";
        let result = parse_sse_event(event_block).unwrap();
        assert!(matches!(result, Some(PromptEvent::Started { .. })));
    }

    #[test]
    fn test_parse_sse_event_done() {
        let event_block = "data:{\"type\":\"done\",\"output_tokens\":100,\"input_tokens\":50,\"latency_ms\":1500,\"finish_reason\":\"stop\"}";
        let result = parse_sse_event(event_block).unwrap();
        assert!(matches!(
            result,
            Some(PromptEvent::Done {
                output_tokens: 100,
                input_tokens: 50,
                ..
            })
        ));
    }

    #[test]
    fn test_parse_sse_event_error() {
        let event_block = "data:{\"type\":\"error\",\"code\":\"rate_limit\",\"message\":\"Too many requests\",\"retryable\":true}";
        let result = parse_sse_event(event_block).unwrap();
        assert!(matches!(
            result,
            Some(PromptEvent::Error {
                retryable: true,
                ..
            })
        ));
    }

    #[test]
    fn test_parse_sse_event_empty() {
        let event_block = "data:";
        let result = parse_sse_event(event_block).unwrap();
        assert!(result.is_none());
    }
}
