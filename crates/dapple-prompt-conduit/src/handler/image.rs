//! Image generation + composite handlers.
//!
//! - `handle_image_prompt` (SSE) is the **base/piece render**: routed from
//!   `handle_prompt` when a prompt's `maker_keys` contain `IMAGE_MAKER_KEY`.
//!   It mirrors `handle_tts_prompt` — resolve config from the prompt's
//!   `engine_body`, generate via the image service, persist as an `image` asset
//!   (draft → chunks → finalize), and emit `PromptEvent::asset`. This is the ONLY
//!   generative image call.
//! - `handle_image_composite` (JSON, `POST /composite`) is the **deterministic
//!   assembly**: it reads a background asset + N generated block assets and
//!   composites them locally (resize, Reinhard color-match, feather, contact
//!   shadow) into one canvas via [`crate::service::image::composite_blocks`].
//!   No model touches the composed pixels — the inviolability principle.

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use dapple_data::model::drafts::{InsertDraftArgs, InsertDraftChunkArgs};
use dapple_data::query::chunks::read_asset_content;
use dapple_data::query::drafts::{finalize_draft, insert_draft_chunks, insert_drafts};
use dapple_data::query::prompts::select_prompt_engine_body_for_maker;
use dapple_data::DbPool;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use crate::error::ConduitError;
use crate::event::PromptEvent;
use crate::handler::prompt::PromptRequest;
use crate::service::image::{
    apply_silhouette_png, apply_silhouette_svg, compose_svg, compose_with_layout, composite_blocks,
    strategy_for, BlockPlacement, CompositeOpts, LayoutParams,
};
use crate::service::AppState;

/// Type alias for the SSE stream used by prompt handlers (matches `prompt.rs`).
type PromptStream = BoxStream<'static, Result<Event, Infallible>>;

/// Lineage/maker key that routes a prompt to the image generator. A guide's
/// `:::inference-profile` declares `.lineage = "image"`; the model id
/// (`gpt-image-1`, etc.) and size live in the engine_body JSON.
pub const IMAGE_MAKER_KEY: &str = "image";

/// Maximum chunk size in bytes (256KB) for asset creation (matches `prompt.rs`).
const MAX_CHUNK_SIZE: usize = 262_144;

/// Config parsed from an image prompt's `engine_body` (JSON).
#[derive(Debug, Default, Deserialize)]
struct ImagePromptConfig {
    /// Which hosted engine to route to (`fal`/`openai`). Absent → the rath→Fal
    /// backend (the primary image path). Selects the backend exactly like
    /// `provider_key` does for LLM prompts.
    provider: Option<String>,
    /// Provider model id; falls back to the image service default.
    model: Option<String>,
    /// Output size `WIDTHxHEIGHT`; falls back to the service default.
    size: Option<String>,
    /// Background mode for models that support it (`transparent`/`opaque`).
    background: Option<String>,
    /// Render quality for the gpt-image family (`low`/`medium`/`high`/`auto`);
    /// falls back to the provider default (`auto`) when unset.
    quality: Option<String>,
    /// Optional generation seed — pinned for reproducibility. Forwarded to
    /// backends that accept one (rath/Fal via provider_config); ignored by others.
    seed: Option<i64>,
    /// Optional negative prompt. Author it in provider_config for Fal models that
    /// support one; ignored by backends without one.
    negative_prompt: Option<String>,
    /// Optional style preamble prepended to the (LLM-assembled) description.
    /// Lives in the inference-profile entity, not in nushell.
    style: Option<String>,
    /// Raw Fal-model `provider_config` passthrough (LayerDiffuse transparency,
    /// guidance, image_size overrides). Forwarded verbatim to the rath backend;
    /// ignored by the OpenAI backend.
    provider_config: Option<Value>,
}

/// Persist raw bytes as an asset of `asset_kind` via the draft → chunks →
/// finalize pipeline. Returns `(asset_id, content_length)`. `mime_type` is
/// stamped on the asset (and served verbatim by the content endpoint). Shared by
/// the image render/compose endpoints (`asset_kind = "image"`, mime `image/png`
/// or `image/svg+xml`) and the A/V compose endpoint (`asset_kind = "document"`,
/// mime `application/json` for an OTIO timeline). `asset_kind` must be one of the
/// seeded `rev.asset_kinds` keys (`image`/`document`/`audio`/`video`/`binary`).
pub(crate) async fn persist_media_asset(
    pool: &DbPool,
    bytes: &[u8],
    asset_kind: &str,
    mime_type: &str,
) -> Result<(Uuid, i64), ConduitError> {
    // Refuse to mint a zero-content asset. `bytes.chunks()` over an empty slice
    // yields no chunks, so without this guard `finalize_draft` would create an
    // asset with content_length 0 — a valid id pointing at nothing, which only
    // detonates downstream (read_asset_content → empty → decode/parse failure)
    // far from the real cause. Fail here instead.
    if bytes.is_empty() {
        return Err(ConduitError::Internal(format!(
            "refusing to persist a 0-byte {asset_kind} asset (empty output)"
        )));
    }

    let drafts = insert_drafts(
        pool,
        InsertDraftArgs {
            asset_kind_keys: vec![asset_kind.to_string()],
            mime_types: vec![mime_type.to_string()],
        },
    )
    .await?;
    let draft = drafts
        .into_iter()
        .next()
        .ok_or_else(|| ConduitError::Internal("no draft created for asset".to_string()))?;

    for chunk in bytes.chunks(MAX_CHUNK_SIZE) {
        insert_draft_chunks(
            pool,
            InsertDraftChunkArgs {
                draft_ids: vec![draft.id],
                data: vec![chunk.to_vec()],
            },
        )
        .await?;
    }

    let content_hash = dapple_data::hash::hash_asset(bytes);
    let finalized = finalize_draft(pool, draft.id, content_hash.as_str()).await?;
    Ok((finalized.asset_id, finalized.content_length))
}

/// Persist raw image bytes as an `image` asset. Thin wrapper over
/// [`persist_media_asset`] — every `image/*` mime is an `image` asset, so an
/// `image/svg+xml` layout document and an `image/png` render share this path.
pub(crate) async fn persist_image_asset(
    pool: &DbPool,
    bytes: &[u8],
    mime_type: &str,
) -> Result<(Uuid, i64), ConduitError> {
    persist_media_asset(pool, bytes, "image", mime_type).await
}

/// Pull the image description from the request (preferred) or the attempt
/// context. The `imagine` command pipes the LLM-assembled scene/block text into
/// `context_data`; we accept any of `prompt`/`description`/`text`/`input`.
fn extract_image_prompt(req_context: &Option<Value>, attempt_context: &Value) -> Option<String> {
    let pick = |v: &Value| {
        v.get("prompt")
            .or_else(|| v.get("description"))
            .or_else(|| v.get("text"))
            .or_else(|| v.get("input"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
    };
    if let Some(rc) = req_context {
        if let Some(s) = pick(rc) {
            if !s.trim().is_empty() {
                return Some(s);
            }
        }
    }
    pick(attempt_context).filter(|s| !s.trim().is_empty())
}

/// Base/piece render: generate a fresh image from a prompt and persist it. SSE so
/// it shares the prompt-stream shape (Started → Asset → Done / Error).
pub async fn handle_image_prompt(
    state: AppState,
    req: PromptRequest,
    request_id: Uuid,
    start: Instant,
) -> Result<Sse<PromptStream>, ConduitError> {
    log::info!(
        "Handling image prompt: request_id={}, prompt_id={}",
        request_id,
        req.prompt_id
    );

    let pool = Arc::clone(&state.pool);

    let stream = async_stream::stream! {
        let started = PromptEvent::started(request_id, "image", req.prompt_id);
        yield Ok(Event::default().data(started.to_sse_data()));

        // 1. Image service availability
        let image_service = match &state.image_service {
            Some(svc) => svc.clone(),
            None => {
                let error = PromptEvent::error("image_unavailable", "Image service is not configured", false);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // 2. Fetch attempt context (description fallback)
        let attempt_context = match state.context_service
            .fetch_context_for_attempt(req.attempt_id)
            .await
        {
            Ok(ctx) => ctx,
            Err(e) => {
                let error = PromptEvent::error("context_error", e.to_string(), false);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // 3. Resolve config from the prompt's engine_body (model/size/background/style)
        let engine_body = select_prompt_engine_body_for_maker(&pool, req.prompt_id, IMAGE_MAKER_KEY)
            .await
            .ok()
            .flatten();
        let cfg: ImagePromptConfig = engine_body
            .as_deref()
            .and_then(|b| serde_json::from_str(b).ok())
            .unwrap_or_default();

        // 4. Assemble the image prompt: style preamble (from the entity) + the
        //    LLM-assembled description (from context_data).
        let description = match extract_image_prompt(&req.context_data, &attempt_context.context_data) {
            Some(d) => d,
            None => {
                let error = PromptEvent::error(
                    "input_error",
                    "No image prompt/description in context_data (expected one of prompt/description/text/input)",
                    false,
                );
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };
        let prompt_text = match cfg.style.as_deref() {
            Some(style) if !style.trim().is_empty() => format!("{}\n\n{}", style.trim(), description),
            _ => description,
        };

        // 5. Generate. Build the request from engine_body; empty model/size are
        //    filled with the service defaults, and `provider` routes to the
        //    matching backend (Fal / OpenAI) the same way the LLM path routes by
        //    provider_key.
        let gen_request = crate::service::image::ImageGenRequest {
            provider: cfg.provider.clone(),
            prompt: prompt_text.clone(),
            model: cfg.model.clone().unwrap_or_default(),
            size: cfg.size.clone().unwrap_or_default(),
            background: cfg.background.clone(),
            quality: cfg.quality.clone(),
            seed: cfg.seed,
            negative_prompt: cfg.negative_prompt.clone(),
            provider_config: cfg.provider_config.clone(),
        };
        let bytes = match image_service
            .generate(gen_request)
            .await
        {
            Ok(b) => b,
            Err(e) => {
                // Image-provider failures are retryable (rate limits, transient 5xx).
                let error = PromptEvent::error("image_error", e.to_string(), true);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // A successful-but-empty generation (e.g. an empty `b64_json` datum) must
        // fail here, at the generating step, rather than minting a 0-byte asset
        // that breaks the later composite. Retryable: a transient provider hiccup.
        if bytes.is_empty() {
            let error = PromptEvent::error("image_error", "image provider returned 0 bytes (empty image)", true);
            yield Ok(Event::default().data(error.to_sse_data()));
            return;
        }

        // 6. Persist as an image asset
        let (asset_id, content_length) = match persist_image_asset(&pool, &bytes, "image/png").await {
            Ok(v) => v,
            Err(e) => {
                let error = PromptEvent::error("persist_error", e.to_string(), false);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        log::info!(
            "Image asset created: asset_id={}, content_length={}",
            asset_id,
            content_length
        );

        // 7. Asset + Done events
        let asset_event = PromptEvent::asset(asset_id, "image", "image/png", content_length, 0.0, 0, prompt_text);
        yield Ok(Event::default().data(asset_event.to_sse_data()));

        let latency_ms = start.elapsed().as_millis() as u64;
        let done = PromptEvent::done(0, 0, latency_ms, "asset_created");
        yield Ok(Event::default().data(done.to_sse_data()));
    };

    Ok(Sse::new(Box::pin(stream)))
}

// ============================================================================
// Deterministic composite endpoint (POST /composite)
// ============================================================================

/// One block to place on the background canvas, identified by its asset id.
#[derive(Debug, Clone, Deserialize)]
pub struct CompositeBlock {
    /// The generated block image asset (its bytes are read from the DB).
    pub asset_id: Uuid,
    /// Target top-left x on the background, in pixels.
    pub x: u32,
    /// Target top-left y on the background, in pixels.
    pub y: u32,
    /// Target cell width; the block is resized to this.
    pub w: u32,
    /// Target cell height; the block is resized to this.
    pub h: u32,
}

fn default_true() -> bool {
    true
}

/// Request body for `POST /composite`.
#[derive(Debug, Clone, Deserialize)]
pub struct ImageCompositeApiRequest {
    /// The background canvas asset (its bytes are read from the DB).
    pub background_asset_id: Uuid,
    /// Blocks to place onto the background, in order.
    pub blocks: Vec<CompositeBlock>,
    /// Reinhard per-channel color match of each block to the background.
    #[serde(default = "default_true")]
    pub color_match: bool,
    /// Border feather radius in pixels (alpha ramp); 0 disables.
    #[serde(default)]
    pub feather: u32,
    /// Drop a soft procedural contact shadow under each block.
    #[serde(default)]
    pub shadow: bool,
    /// Optional output canvas width in px. When set (with height), the background is
    /// Lanczos-upscaled to this size before compositing so the pieces land at higher
    /// resolution. Default (unset) keeps the canvas at the background's native size.
    #[serde(default)]
    pub canvas_width: Option<u32>,
    /// Optional output canvas height in px (see `canvas_width`).
    #[serde(default)]
    pub canvas_height: Option<u32>,
}

/// Response body for `POST /composite`.
#[derive(Debug, Clone, Serialize)]
pub struct ImageCompositeApiResponse {
    pub asset_id: Uuid,
    pub asset_kind: String,
    pub mime_type: String,
    pub content_length: i64,
    /// Output dimensions (the background's dimensions).
    pub width: u32,
    pub height: u32,
}

/// `POST /composite` — deterministic, local assembly of generated pieces. Reads
/// the background + each block asset, composites them with color-grade/feather/
/// shadow, and persists the result as a new image asset. No model is involved;
/// existing pixels are never reprocessed through a generative service.
pub async fn handle_image_composite(
    State(state): State<AppState>,
    Json(req): Json<ImageCompositeApiRequest>,
) -> Result<Json<ImageCompositeApiResponse>, ConduitError> {
    // 1. Read the background canvas bytes. `read_asset_content` returns an empty
    //    Vec (not an error) when an asset has no chunks — a wrong/unknown asset id
    //    or one that was never finalized. Catch that here with a precise message
    //    instead of letting it fall through to an opaque `decode` failure.
    let background = read_asset_content(&state.pool, req.background_asset_id).await?;
    if background.is_empty() {
        return Err(ConduitError::ImageError(format!(
            "background asset {} has no content (0 bytes) — wrong asset id or it was never finalized",
            req.background_asset_id
        )));
    }

    // 2. Read each block's bytes and build placements.
    let mut placements = Vec::with_capacity(req.blocks.len());
    for (i, b) in req.blocks.iter().enumerate() {
        let png_bytes = read_asset_content(&state.pool, b.asset_id).await?;
        if png_bytes.is_empty() {
            return Err(ConduitError::ImageError(format!(
                "block[{i}] asset {} has no content (0 bytes) — wrong asset id or it was never finalized",
                b.asset_id
            )));
        }
        placements.push(BlockPlacement {
            png_bytes,
            x: b.x,
            y: b.y,
            w: b.w,
            h: b.h,
        });
    }

    let opts = CompositeOpts {
        color_match: req.color_match,
        feather: req.feather,
        shadow: req.shadow,
    };
    // Optional output-canvas size: only an upscale when BOTH dims are given.
    let canvas = match (req.canvas_width, req.canvas_height) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => None,
    };
    let block_count = req.blocks.len();
    let background_asset_id = req.background_asset_id;

    // 3. Composite on a blocking thread — pixel work is CPU-bound and would
    //    otherwise stall the async runtime. Purely deterministic.
    let (bytes, width, height) = tokio::task::spawn_blocking(move || {
        composite_blocks(&background, &placements, opts, canvas)
    })
    .await
    .map_err(|e| ConduitError::Internal(format!("composite task panicked: {e}")))??;

    // 4. Persist the result as a new image asset.
    let (asset_id, content_length) = persist_image_asset(&state.pool, &bytes, "image/png").await?;

    log::info!(
        "Image composite complete: background={}, blocks={}, new_asset={}, size={}x{}",
        background_asset_id,
        block_count,
        asset_id,
        width,
        height
    );

    Ok(Json(ImageCompositeApiResponse {
        asset_id,
        asset_kind: "image".to_string(),
        mime_type: "image/png".to_string(),
        content_length,
        width,
        height,
    }))
}

// ============================================================================
// Strategy-driven compose endpoint (POST /compose)
// ============================================================================

/// One piece to lay out + composite. Only `asset_id` is required; `shape`/`weight`
/// are forward-compat hints (the `panorama` strategy measures aspect from the
/// pieces' pixels and ignores them). `label` names the piece's SVG layer when
/// `output_format = "svg"` (ignored by the raster path).
#[derive(Debug, Clone, Deserialize)]
pub struct ComposePiece {
    /// The generated piece image asset (its bytes are read from the DB).
    pub asset_id: Uuid,
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub weight: Option<f32>,
    /// Human layer name for the SVG path (e.g. `"Scene 1"`). Names the piece's
    /// `<g>` layer in SVG mode; ignored by the raster path.
    #[serde(default)]
    pub label: Option<String>,
}

fn default_strategy() -> String {
    "panorama".to_string()
}

/// Request body for `POST /compose`. Unlike `/composite`, the caller does NOT
/// position pieces — it names a layout `strategy` and the server measures each
/// piece's content and arranges them deterministically.
#[derive(Debug, Clone, Deserialize)]
pub struct ImageComposeApiRequest {
    /// The background canvas asset (its bytes are read from the DB).
    pub background_asset_id: Uuid,
    /// Pieces to lay out + composite.
    pub pieces: Vec<ComposePiece>,
    /// Layout strategy name (`panorama`). Unknown names are a 4xx.
    #[serde(default = "default_strategy")]
    pub strategy: String,
    /// Strategy knobs; a partial object overrides only those keys (rest default).
    #[serde(default)]
    pub params: LayoutParams,
    #[serde(default = "default_true")]
    pub color_match: bool,
    #[serde(default)]
    pub feather: u32,
    #[serde(default)]
    pub shadow: bool,
    /// Optional output canvas size (Lanczos-upscales the background; see `/composite`).
    #[serde(default)]
    pub canvas_width: Option<u32>,
    #[serde(default)]
    pub canvas_height: Option<u32>,
    /// Output serialization: `"png"` (default) flattens to a raster; `"svg"` emits
    /// a self-contained SVG layout document where each piece is a named, movable
    /// layer for hand-finishing. Same deterministic layout either way.
    #[serde(default)]
    pub output_format: Option<String>,
    /// Optional silhouette SVG asset: the composed spread is clipped to the
    /// mark (contain-fit, centered) — the video-quilt reveal's "SVG overlay
    /// trick". Raster output gets a transparent outside; SVG output gets an
    /// inline luminance mask the operator can remove.
    #[serde(default)]
    pub silhouette_asset_id: Option<Uuid>,
}

/// `POST /compose` — strategy-driven assembly: read the background + each piece,
/// trim each piece to its real content, arrange them with the named deterministic
/// strategy, composite, and persist as a new image asset. No model is involved —
/// existing pixels are never reprocessed through a generative service.
pub async fn handle_image_compose(
    State(state): State<AppState>,
    Json(req): Json<ImageComposeApiRequest>,
) -> Result<Json<ImageCompositeApiResponse>, ConduitError> {
    let background = read_asset_content(&state.pool, req.background_asset_id).await?;
    if background.is_empty() {
        return Err(ConduitError::ImageError(format!(
            "background asset {} has no content (0 bytes) — wrong asset id or it was never finalized",
            req.background_asset_id
        )));
    }

    let strategy = strategy_for(&req.strategy).ok_or_else(|| {
        ConduitError::InvalidRequest(format!("unknown layout strategy '{}'", req.strategy))
    })?;

    if req.pieces.is_empty() {
        return Err(ConduitError::InvalidRequest(
            "compose requires at least one piece".to_string(),
        ));
    }

    let mut piece_pngs = Vec::with_capacity(req.pieces.len());
    let mut labels = Vec::with_capacity(req.pieces.len());
    for (i, p) in req.pieces.iter().enumerate() {
        let bytes = read_asset_content(&state.pool, p.asset_id).await?;
        if bytes.is_empty() {
            return Err(ConduitError::ImageError(format!(
                "piece[{i}] asset {} has no content (0 bytes) — wrong asset id or it was never finalized",
                p.asset_id
            )));
        }
        piece_pngs.push(bytes);
        labels.push(p.label.clone());
    }

    // Optional silhouette (the reveal clip mark).
    let silhouette = match req.silhouette_asset_id {
        Some(id) => {
            let bytes = read_asset_content(&state.pool, id).await?;
            if bytes.is_empty() {
                return Err(ConduitError::ImageError(format!(
                    "silhouette asset {id} has no content (0 bytes) — wrong asset id or it was never finalized"
                )));
            }
            Some(bytes)
        }
        None => None,
    };

    let opts = CompositeOpts {
        color_match: req.color_match,
        feather: req.feather,
        shadow: req.shadow,
    };
    let canvas = match (req.canvas_width, req.canvas_height) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => None,
    };
    let params = req.params;
    let piece_count = req.pieces.len();
    let background_asset_id = req.background_asset_id;

    // `output_format` selects the serialization of the SAME deterministic layout:
    // "svg" emits a self-contained, hand-editable layout document; anything else
    // (default) flattens to a raster PNG. Unknown values fall through to PNG.
    let as_svg = req
        .output_format
        .as_deref()
        .is_some_and(|f| f.eq_ignore_ascii_case("svg"));

    // Compose on a blocking thread — pixel work is CPU-bound. Deterministic.
    let (bytes, mime_type, width, height) = tokio::task::spawn_blocking(move || {
        if as_svg {
            let (b, w, h) = compose_svg(
                &background,
                &piece_pngs,
                &labels,
                strategy.as_ref(),
                &params,
                opts,
                canvas,
            )?;
            let b = match &silhouette {
                Some(svg) => apply_silhouette_svg(&b, svg, w, h)?,
                None => b,
            };
            Ok::<_, ConduitError>((b, "image/svg+xml", w, h))
        } else {
            let (b, w, h) = compose_with_layout(
                &background,
                &piece_pngs,
                strategy.as_ref(),
                &params,
                opts,
                canvas,
            )?;
            let b = match &silhouette {
                Some(svg) => apply_silhouette_png(&b, svg)?,
                None => b,
            };
            Ok((b, "image/png", w, h))
        }
    })
    .await
    .map_err(|e| ConduitError::Internal(format!("compose task panicked: {e}")))??;

    let (asset_id, content_length) = persist_image_asset(&state.pool, &bytes, mime_type).await?;

    log::info!(
        "Image compose complete: background={}, pieces={}, strategy={}, format={}, new_asset={}, size={}x{}",
        background_asset_id,
        piece_count,
        req.strategy,
        mime_type,
        asset_id,
        width,
        height
    );

    Ok(Json(ImageCompositeApiResponse {
        asset_id,
        asset_kind: "image".to_string(),
        mime_type: mime_type.to_string(),
        content_length,
        width,
        height,
    }))
}
