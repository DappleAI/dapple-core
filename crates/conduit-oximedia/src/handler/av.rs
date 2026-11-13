//! Audio + video compose handler (`POST /compose-av`).
//!
//! The A/V analog of the image `POST /compose` (`handle_image_compose`): it takes
//! already-generated audio/video **piece assets** and assembles them into an
//! editable OpenTimelineIO (OTIO) timeline document — the deterministic,
//! no-model assembly step. Unlike the image path there are no pixels to read: the
//! timeline references each piece by its asset content URL (an OTIO
//! `ExternalReference`), so the handler only needs each asset's metadata
//! (existence + `asset_kind`) plus a caller-supplied duration. A model never
//! touches the media (the inviolability principle); the operator finishes the cut
//! in a real NLE.

use axum::{extract::State, Json};
use dapple_data::query::assets::select_asset_by_object_id;
use dapple_data::query::chunks::read_asset_content;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::OximediaError;
use crate::persist::persist_media_asset;
use crate::service::audio_analysis::{Shift, SongMap};
use crate::service::av::{
    compose_timeline, compose_timeline_from_doc, interrupt_caps, referenced_assets, resolve_starts,
    strategy_for, validate_composition, AudioMixCompositor, ClipKind, CompositionDoc, Compositor,
    MixInput, Relate, RelateMode, ShiftMarker, TimelineFormat, TimelineParams, TimelinePiece,
};
use crate::service::wav::{decode_wav, trim_edge_silence, DecodedAudio};
use crate::service::AppState;

/// One generated piece to place on the timeline.
#[derive(Debug, Clone, Deserialize)]
pub struct ComposeAvPiece {
    /// The generated audio/video piece asset (metadata is read; bytes are not).
    pub asset_id: Uuid,
    /// Clip name shown in the NLE. Defaults to `clip N`.
    #[serde(default)]
    pub name: Option<String>,
    /// Track family override (`video`/`audio`). When omitted it's derived from
    /// the asset's `asset_kind`, so a caller normally doesn't set it.
    #[serde(default)]
    pub kind: Option<String>,
    /// Clip length in seconds. Required — the producer (TTS/video-gen) knows it,
    /// and v1 deliberately does not probe media containers for duration. (The
    /// `wav` mix path uses the DECODED length instead, so this may be approximate
    /// there.)
    pub duration_seconds: f64,
    /// Mix path (`output: "wav"`) only: how this clip relates to a PRIOR clip in
    /// time — the overlapping-conversation chain. Ignored by the document path.
    #[serde(default)]
    pub relate: Option<WireRelate>,
    /// Mix path only: linear playback gain (1.0 = unity; < 1 = the mutter-down).
    #[serde(default)]
    pub gain: Option<f64>,
}

/// How a piece relates to a prior piece on the conversation timeline (mix path
/// only). Both anchor to the prior clip's END; `mode` flips the sign convention.
#[derive(Debug, Clone, Deserialize)]
pub struct WireRelate {
    /// Index of the anchor piece in `pieces` — MUST be earlier than this piece.
    pub to: usize,
    /// `consecutive` (follows) or `interrupting` (overlaps). Default `consecutive`.
    #[serde(default = "default_relate_mode")]
    pub mode: String,
    /// Seconds; interpreted per `mode` (`consecutive`: gap after the anchor end,
    /// negative = slight overlap; `interrupting`: seconds before the anchor end).
    #[serde(default)]
    pub offset_seconds: f64,
}

fn default_relate_mode() -> String {
    "consecutive".to_string()
}

fn default_reel_name() -> String {
    "Untitled Reel".to_string()
}
fn default_fps_num() -> i32 {
    24
}
fn default_fps_den() -> i32 {
    1
}
fn default_sample_rate() -> u32 {
    48_000
}
fn default_av_strategy() -> String {
    "sequence".to_string()
}
fn default_av_format() -> String {
    "otio".to_string()
}
fn default_av_output() -> String {
    "document".to_string()
}

/// Request body for `POST /compose-av`. The caller does NOT position clips — it
/// names an arrangement `strategy` and the server lays clips out deterministically.
#[derive(Debug, Clone, Deserialize)]
pub struct ComposeAvRequest {
    /// Timeline/project name.
    #[serde(default = "default_reel_name")]
    pub name: String,
    /// Frame-rate numerator (24 for 24fps; 24000 for 23.976 with `fps_den = 1001`).
    #[serde(default = "default_fps_num")]
    pub fps_num: i32,
    /// Frame-rate denominator (1 for integer rates).
    #[serde(default = "default_fps_den")]
    pub fps_den: i32,
    /// Timeline audio sample rate (Hz).
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    /// Optional prefix for media references, e.g. `https://api.dapple.x`. The
    /// per-clip reference becomes `{media_base}/api/assets/{id}/content`; empty
    /// yields a root-relative `/api/assets/{id}/content`.
    #[serde(default)]
    pub media_base: Option<String>,
    /// Arrangement strategy name (`sequence`). Unknown names are a 4xx.
    #[serde(default = "default_av_strategy")]
    pub strategy: String,
    /// Strategy knobs; a partial object overrides only those keys.
    #[serde(default)]
    pub params: TimelineParams,
    /// Output format (`otio`/`json`/`fcpxml`/`edl`). Unknown values are a 4xx;
    /// a recognized-but-unshippable format (`aaf`) is a 4xx with an explicit
    /// "not supported at oximedia 0.1.8" message.
    #[serde(default = "default_av_format")]
    pub format: String,
    /// Optional song-map asset (the `document`/JSON output of `/analyze-audio`).
    /// When set, its typed `shifts` are placed on the timeline as markers — the
    /// analysis → timeline link. Absent ⇒ no markers (identical to before).
    #[serde(default)]
    pub song_map_asset_id: Option<Uuid>,
    /// Deliverable: `document` (default) — an editable OTIO/FCPXML/EDL timeline
    /// that *references* clips — or `wav`, which mixes the clip AUDIO into one
    /// overlapping-conversation WAV (reads clip bytes; honors per-piece
    /// `relate`/`gain`; `strategy`/`format`/`fps` are ignored).
    #[serde(default = "default_av_output")]
    pub output: String,
    /// `strategy: "document"` only: the guide-compiled composition document
    /// (version 3). The backend validates it and persists it for the renderer
    /// (`format: "render"`) or exports it as an editable timeline
    /// (`format: otio/fcpxml/edl`). All genre policy — what the layers mean,
    /// where they sit, when they move — was compiled guide-side.
    #[serde(default)]
    pub document: Option<serde_json::Value>,
    /// Pieces to arrange, in order. Not used by `strategy: "document"` (the
    /// document already references its assets).
    #[serde(default)]
    pub pieces: Vec<ComposeAvPiece>,
}

/// Response body for `POST /compose-av`.
#[derive(Debug, Clone, Serialize)]
pub struct ComposeAvResponse {
    pub asset_id: Uuid,
    /// `document` for the timeline path, or `audio` for the `wav` mix path.
    pub asset_kind: String,
    pub mime_type: String,
    pub content_length: i64,
    /// The serialization format used (`otio`).
    pub format: String,
    /// Number of clips placed on the timeline.
    pub clip_count: usize,
}

/// `POST /compose-av` — deterministic assembly of generated A/V pieces into an
/// editable OTIO timeline document. Validates each piece asset's existence +
/// kind, arranges them with the named strategy, and persists the timeline as a
/// `document` asset. No media bytes are read or re-encoded.
pub async fn handle_compose_av(
    State(state): State<AppState>,
    Json(req): Json<ComposeAvRequest>,
) -> Result<Json<ComposeAvResponse>, OximediaError> {
    // Document path: a guide-compiled composition document (version 3). The
    // backend is a pure mechanism here — validate, persist for the renderer,
    // or export as an editable timeline. No pieces, no strategy math.
    if req.strategy.trim().eq_ignore_ascii_case("document") {
        return handle_document_av(&state, &req).await;
    }
    if req.document.is_some() {
        return Err(OximediaError::InvalidRequest(
            "a composition document requires strategy 'document'".to_string(),
        ));
    }

    if req.pieces.is_empty() {
        return Err(OximediaError::InvalidRequest(
            "compose-av requires at least one piece".to_string(),
        ));
    }

    // Mixed-audio render path: overlapping single-voice clips → one WAV. Reads clip
    // bytes and honors per-piece relate/gain; the OTIO-document setup below (fps,
    // strategy, format, media_ref) does not apply.
    if req.output.eq_ignore_ascii_case("wav") {
        return handle_mix_av(&state, &req).await;
    }

    // `format: "render"` is the renderer's composition manifest — meaningless
    // for the classic sequence document path.
    if req.format.trim().eq_ignore_ascii_case("render") {
        return Err(OximediaError::InvalidRequest(
            "format 'render' requires strategy 'document'".to_string(),
        ));
    }

    if req.fps_num <= 0 || req.fps_den <= 0 {
        return Err(OximediaError::InvalidRequest(
            "fps_num and fps_den must be positive".to_string(),
        ));
    }

    let strategy = strategy_for(&req.strategy).ok_or_else(|| {
        OximediaError::InvalidRequest(format!("unknown arrangement strategy '{}'", req.strategy))
    })?;
    let format = TimelineFormat::parse(&req.format).ok_or_else(|| {
        // Distinguish a recognized-but-gated format (e.g. AAF at oximedia 0.1.8)
        // from a genuinely unknown one so the caller gets an honest reason.
        if TimelineFormat::is_known_unsupported(&req.format) {
            OximediaError::InvalidRequest(format!(
                "format '{}' not supported at oximedia 0.1.8",
                req.format.trim().to_ascii_lowercase()
            ))
        } else {
            OximediaError::InvalidRequest(format!("unknown format '{}'", req.format))
        }
    })?;

    let base = req
        .media_base
        .as_deref()
        .unwrap_or("")
        .trim_end_matches('/');
    let fps_ratio = f64::from(req.fps_num) / f64::from(req.fps_den);

    // Resolve each piece to a TimelinePiece: validate the asset exists, derive its
    // track family, convert its duration to frames, and build the media reference.
    let mut pieces = Vec::with_capacity(req.pieces.len());
    for (i, p) in req.pieces.iter().enumerate() {
        if !(p.duration_seconds.is_finite() && p.duration_seconds > 0.0) {
            return Err(OximediaError::InvalidRequest(format!(
                "piece[{i}] needs a positive, finite duration_seconds"
            )));
        }

        // Metadata-only lookup — validates existence and gives us the asset kind
        // (no chunk read). A missing/unknown id surfaces as a 404.
        let asset = select_asset_by_object_id(&state.pool, p.asset_id)
            .await
            .map_err(|_| {
                OximediaError::NotFound(format!(
                    "piece[{i}] asset {} not found (wrong id or never finalized)",
                    p.asset_id
                ))
            })?;

        let kind_str = p.kind.clone().unwrap_or_else(|| asset.asset_kind.clone());
        let kind = match kind_str.to_ascii_lowercase().as_str() {
            "video" => ClipKind::Video,
            "audio" => ClipKind::Audio,
            other => {
                return Err(OximediaError::InvalidRequest(format!(
                    "piece[{i}] asset {} has unsupported kind '{other}' (want video or audio)",
                    p.asset_id
                )))
            }
        };

        let frames = (p.duration_seconds * fps_ratio).round() as i64;
        let frames = frames.max(1);
        let media_ref = format!("{base}/api/assets/{}/content", p.asset_id);
        let name = p.name.clone().unwrap_or_else(|| format!("clip {}", i + 1));

        pieces.push(TimelinePiece {
            kind,
            name,
            media_ref,
            duration_frames: frames,
        });
    }

    // Optionally load the song map and turn its shifts into timeline markers.
    // This is the analysis → timeline link: `/analyze-audio` produced the typed
    // shifts; here they become OTIO/FCPXML markers on the composed timeline.
    let markers = load_song_map_markers(&state, req.song_map_asset_id).await?;

    let clip_count = pieces.len();
    let name = req.name.clone();
    let fps_num = req.fps_num;
    let fps_den = req.fps_den;
    let sample_rate = req.sample_rate;
    let params = req.params;

    // Build on a blocking thread to keep the (synchronous) OxiMedia work off the
    // async reactor and match the image compose pattern. Pure + deterministic.
    let (bytes, mime) = tokio::task::spawn_blocking(move || {
        compose_timeline(
            &name,
            fps_num,
            fps_den,
            sample_rate,
            &pieces,
            &markers,
            strategy.as_ref(),
            &params,
            format,
        )
    })
    .await
    .map_err(|e| OximediaError::Internal(format!("compose-av task panicked: {e}")))??;

    let (asset_id, content_length) =
        persist_media_asset(&state.pool, &bytes, "document", mime).await?;

    log::info!(
        "AV compose complete: pieces={}, strategy={}, format={}, new_asset={}, bytes={}",
        clip_count,
        req.strategy,
        req.format,
        asset_id,
        content_length
    );

    Ok(Json(ComposeAvResponse {
        asset_id,
        asset_kind: "document".to_string(),
        mime_type: mime.to_string(),
        content_length,
        format: req.format,
        clip_count,
    }))
}

/// Mix path for `POST /compose-av` with `output: "wav"` — sum the pieces' audio
/// into one overlapping-conversation WAV. Reads each clip's bytes, resolves the
/// relative-chain placement (overlaps allowed), mixes on a blocking thread, and
/// persists an `audio` asset.
async fn handle_mix_av(
    state: &AppState,
    req: &ComposeAvRequest,
) -> Result<Json<ComposeAvResponse>, OximediaError> {
    // Read each clip's WAV bytes + collect its relation and gain.
    let mut clip_bytes: Vec<Vec<u8>> = Vec::with_capacity(req.pieces.len());
    let mut relates: Vec<Option<Relate>> = Vec::with_capacity(req.pieces.len());
    let mut gains: Vec<f32> = Vec::with_capacity(req.pieces.len());
    for (i, p) in req.pieces.iter().enumerate() {
        let bytes = read_asset_content(&state.pool, p.asset_id)
            .await
            .map_err(|_| {
                OximediaError::NotFound(format!(
                    "piece[{i}] asset {} not found (wrong id or never finalized)",
                    p.asset_id
                ))
            })?;
        if bytes.is_empty() {
            return Err(OximediaError::NotFound(format!(
                "piece[{i}] asset {} has no content (never finalized?)",
                p.asset_id
            )));
        }
        clip_bytes.push(bytes);

        let rel = match &p.relate {
            None => None,
            Some(w) => {
                if w.to >= i {
                    return Err(OximediaError::InvalidRequest(format!(
                        "piece[{i}] relate.to={} must reference an earlier piece",
                        w.to
                    )));
                }
                Some(Relate {
                    to: w.to,
                    mode: RelateMode::parse(&w.mode),
                    offset_seconds: w.offset_seconds,
                })
            }
        };
        relates.push(rel);
        gains.push(p.gain.unwrap_or(1.0) as f32);
    }

    let clip_count = req.pieces.len();

    // Decode + resolve + mix off the async reactor (CPU-bound), matching the
    // document path's spawn_blocking.
    let (bytes, mime) =
        tokio::task::spawn_blocking(move || -> Result<(Vec<u8>, &'static str), String> {
            let decoded: Vec<DecodedAudio> = clip_bytes
                .iter()
                .enumerate()
                .map(|(i, b)| decode_wav(b).map_err(|e| format!("piece[{i}]: {e}")))
                .collect::<Result<_, _>>()?;
            // Trim head/tail TTS silence so timing offsets (gaps, overlaps, barge-ins)
            // act on real speech boundaries instead of the padding — otherwise a
            // sub-second overlap lands on silence and every preset sounds the same.
            let decoded: Vec<DecodedAudio> = decoded.iter().map(trim_edge_silence).collect();
            let durations: Vec<f64> = decoded.iter().map(DecodedAudio::duration_seconds).collect();
            let starts = resolve_starts(&relates, &durations)?;
            // A floor-taking `interrupting` relation chops the interrupted clip where
            // the interrupter barges in (a backchannel `consecutive` overlap does not).
            let caps = interrupt_caps(&relates, &starts, &durations);
            // Mix at the first clip's rate (TTS clips are uniform); others resample.
            let out_rate = decoded.first().map_or(24_000, |d| d.sample_rate);
            let inputs: Vec<MixInput> = decoded
                .iter()
                .zip(starts.iter())
                .zip(gains.iter())
                .zip(caps.iter())
                .map(
                    |(((d, &start_seconds), &gain), &end_cap_seconds)| MixInput {
                        audio: d,
                        start_seconds,
                        gain,
                        end_cap_seconds,
                    },
                )
                .collect();
            AudioMixCompositor::default().render(&inputs, out_rate)
        })
        .await
        .map_err(|e| OximediaError::Internal(format!("mix-av task panicked: {e}")))?
        .map_err(OximediaError::AvError)?;

    let (asset_id, content_length) =
        persist_media_asset(&state.pool, &bytes, "audio", mime).await?;

    log::info!(
        "AV mix complete: clips={}, new_asset={}, bytes={}",
        clip_count,
        asset_id,
        content_length
    );

    Ok(Json(ComposeAvResponse {
        asset_id,
        asset_kind: "audio".to_string(),
        mime_type: mime.to_string(),
        content_length,
        format: "wav".to_string(),
        clip_count,
    }))
}

/// Document path for `POST /compose-av` (`strategy: "document"`) — a
/// guide-compiled composition document (version 3). Pure mechanism: validate
/// the document, check every referenced asset exists, then either persist the
/// document itself as the renderer's manifest (`format: "render"`) or export
/// it as the operator's editable timeline (`format: otio/fcpxml/edl`). The
/// backend learns nothing about the genre that produced it.
async fn handle_document_av(
    state: &AppState,
    req: &ComposeAvRequest,
) -> Result<Json<ComposeAvResponse>, OximediaError> {
    let value = req.document.clone().ok_or_else(|| {
        OximediaError::InvalidRequest(
            "strategy 'document' requires the composition document in `document`".to_string(),
        )
    })?;
    let doc: CompositionDoc = serde_json::from_value(value).map_err(|e| {
        OximediaError::InvalidRequest(format!("composition document does not parse: {e}"))
    })?;
    validate_composition(&doc)?;

    let render_format = req.format.trim().eq_ignore_ascii_case("render");
    let doc_format = if render_format {
        None
    } else {
        Some(TimelineFormat::parse(&req.format).ok_or_else(|| {
            OximediaError::InvalidRequest(format!(
                "unknown format '{}' (strategy 'document' takes otio/fcpxml/edl/render)",
                req.format
            ))
        })?)
    };

    // Fail on a dangling asset reference at compose time — cheaper than a
    // render job dying minutes in. Metadata-only lookups.
    for (asset_id, what) in referenced_assets(&doc) {
        select_asset_by_object_id(&state.pool, asset_id)
            .await
            .map_err(|_| {
                OximediaError::NotFound(format!(
                    "{what} asset {asset_id} not found (wrong id or never finalized)"
                ))
            })?;
    }

    let clip_count = doc
        .layers
        .iter()
        .map(|l| l.segments.len().max(1))
        .sum::<usize>()
        + 2; // outro + audio

    let (bytes, mime) = match doc_format {
        // The renderer's manifest: the validated document itself, re-serialized.
        None => {
            let bytes = serde_json::to_vec_pretty(&doc).map_err(|e| {
                OximediaError::Internal(format!("failed to serialize composition document: {e}"))
            })?;
            (bytes, "application/json")
        }
        // The operator's editable timeline.
        Some(fmt) => {
            let base = req.media_base.clone().unwrap_or_default();
            let sample_rate = req.sample_rate;
            tokio::task::spawn_blocking(move || {
                compose_timeline_from_doc(&doc, &base, sample_rate, fmt)
            })
            .await
            .map_err(|e| {
                OximediaError::Internal(format!("compose-av document task panicked: {e}"))
            })??
        }
    };

    let (asset_id, content_length) =
        persist_media_asset(&state.pool, &bytes, "document", mime).await?;

    log::info!(
        "AV document compose complete: format={}, clips={}, new_asset={}, bytes={}",
        req.format,
        clip_count,
        asset_id,
        content_length
    );

    Ok(Json(ComposeAvResponse {
        asset_id,
        asset_kind: "document".to_string(),
        mime_type: mime.to_string(),
        content_length,
        format: req.format.clone(),
        clip_count,
    }))
}

/// Load the song-map asset (if any) and map its shifts to timeline markers.
///
/// Returns an empty Vec when `song_map_asset_id` is `None` (the timeline gets no
/// markers — identical to the pre-song-map behavior). A missing asset is a 404; a
/// non-song-map / unparseable document is a 4xx.
async fn load_song_map_markers(
    state: &AppState,
    song_map_asset_id: Option<Uuid>,
) -> Result<Vec<ShiftMarker>, OximediaError> {
    let Some(song_map_id) = song_map_asset_id else {
        return Ok(Vec::new());
    };

    let bytes = read_asset_content(&state.pool, song_map_id)
        .await
        .map_err(|_| {
            OximediaError::NotFound(format!(
                "song_map asset {song_map_id} not found (wrong id or never finalized)"
            ))
        })?;
    if bytes.is_empty() {
        return Err(OximediaError::NotFound(format!(
            "song_map asset {song_map_id} has no content (never finalized?)"
        )));
    }

    let song_map: SongMap = serde_json::from_slice(&bytes).map_err(|e| {
        OximediaError::InvalidRequest(format!(
            "song_map asset {song_map_id} is not a valid song map: {e}"
        ))
    })?;

    Ok(song_map.shifts.iter().map(shift_to_marker).collect())
}

/// Map one analysis [`Shift`] to a timeline [`ShiftMarker`]. The name is the
/// shift's stable kind label; the comment records the source stem + salience for
/// the operator (a plain string, so it stays deterministic).
fn shift_to_marker(shift: &Shift) -> ShiftMarker {
    let stem = shift.stem.as_deref().unwrap_or("fused");
    ShiftMarker {
        t: shift.t,
        name: shift.kind.label().to_string(),
        comment: format!("{stem} (strength {:.3})", shift.strength),
    }
}
