//! The generic layered-composition renderer.
//!
//! Consumes a [`CompositionDoc`] (version 3) and produces the final mp4 with
//! one ffmpeg run. The document speaks plain NLE vocabulary and nothing else:
//!
//! - a black base plane at output size;
//! - **layers** in z order, each a video source with
//!   - *time-remap segments* (play a source range at a timeline instant,
//!     freeze-frame until the next — `trim` + `tpad=stop_mode=clone` chains,
//!     concat'd per layer; frame 0 holds before a late first segment),
//!   - *placement keyframes* (a step function of `t`: one keyframe renders a
//!     static `overlay`, several render a piecewise position expression — the
//!     layer is scaled once to its largest keyframed rect and smaller
//!     keyframes center it),
//!   - an optional *matte* (an SVG/image asset rasterized to a luma matte,
//!     optionally inverted, optionally drifting on a slow Lissajous — the
//!     drift is crop expressions of `t` over a slightly oversized raster,
//!     never per-frame re-rasterization);
//! - time-windowed **color spans** (`colorbalance` + `eq` gated with
//!   `enable='between(t,a,b)'`);
//! - a hard **cut to a held still** (`outro`), held to the end;
//! - a single **audio** track.
//!
//! POLICY LIVES UPSTREAM. This module knows nothing about songs, stems, dots,
//! masks-as-logos, reveals, or any guide: GUIDE tasks are the policy compilers
//! that turn genre semantics (e.g. the video-quilt) into this document, which
//! conduit-oximedia validates and persists (`POST /compose-av`,
//! `strategy: "document"`). Do not add genre vocabulary here — if a new visual
//! idea can't be expressed with these primitives, extend the primitives
//! generically (a new keyframe channel, a new matte mode), never with a named
//! concept from one guide.
//!
//! `build_composition_filter` is pure (unit-tested like `build_xfade_filter`);
//! [`render_composition`] does the I/O around it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dapple_data::query::chunks::read_asset_content;
use dapple_data::DbPool;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::media::probe_duration;
use super::svg_mask::rasterize_mask;
use crate::error::MediaGatewayError;
use crate::persist::persist_media_asset;

/// Hard output-length ceiling (seconds). Fail loud past it — a runaway render
/// would occupy the pod for tens of minutes.
const MAX_RENDER_SECONDS: f64 = 240.0;
/// A drifting matte is rasterized this much larger than its layer so the
/// drift crop always stays inside it.
const MATTE_OVERSIZE: f64 = 1.05;

// ── Wire types (the CompositionDoc contract with conduit-oximedia) ──────────

/// The renderer's input: a generic layered composition, version 3. Compiled
/// by a guide task, validated + persisted as a `document` asset by
/// conduit-oximedia (`POST /compose-av`, `strategy: "document"`,
/// `format: "render"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionDoc {
    /// Document schema version. This renderer accepts exactly 3; the retired
    /// version-2 "progression manifest" is rejected with guidance.
    pub version: u32,
    pub name: String,
    pub fps_num: i32,
    pub fps_den: i32,
    pub width: u32,
    pub height: u32,
    /// Output duration (seconds); the outro still holds to it.
    pub duration_seconds: f64,
    pub audio: AudioSource,
    /// The hard cut from the composed layers to a held still.
    pub outro: OutroStill,
    /// Layers in ascending z order (later paints over earlier).
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub color_spans: Vec<ColorSpan>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AudioSource {
    pub asset_id: Uuid,
    pub gain: f64,
}

/// Hard cut at `at_s` to a still image, held to the document's end.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct OutroStill {
    pub at_s: f64,
    pub asset_id: Uuid,
}

/// One video layer of the composition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    /// Display label for logs/errors only.
    pub name: String,
    pub asset_id: Uuid,
    /// Loop the source to cover its span (input-level `-stream_loop`).
    #[serde(default)]
    pub loop_source: bool,
    /// Time-remap schedule. Empty ⇒ continuous playback from source start.
    #[serde(default)]
    pub segments: Vec<Segment>,
    /// Placement keyframes (step function of `t`). Empty ⇒ the full canvas.
    #[serde(default)]
    pub placement: Vec<PlacementKey>,
    #[serde(default)]
    pub matte: Option<Matte>,
}

/// Play `duration_s` of source starting `source_in_s`, at timeline `at_s`;
/// the last frame freezes until the next segment (or the outro cut).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Segment {
    pub at_s: f64,
    pub source_in_s: f64,
    pub duration_s: f64,
}

/// The layer's rect from `at_s` onward (pixels, top-left origin). The layer is
/// scaled once to the largest keyframed rect; a smaller keyframe centers that
/// scaled frame on its own rect (the overflow hides behind whatever paints
/// above it).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlacementKey {
    pub at_s: f64,
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
}

/// A luma matte cut from a rasterizable asset (SVG or image): painted pixels
/// keep the layer, unpainted drop it — inverted when `invert`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Matte {
    pub asset_id: Uuid,
    #[serde(default)]
    pub invert: bool,
    /// Slow deterministic wander of the matte over the layer.
    #[serde(default)]
    pub drift: Option<MatteDrift>,
}

/// Lissajous drift parameters: the matte is rasterized oversized and cropped
/// with `x/y` expressions of `t`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MatteDrift {
    /// Horizontal angular rate (radians/second).
    pub rate_x: f64,
    /// Vertical angular rate (radians/second).
    pub rate_y: f64,
    /// Fraction of the available oversize margin the wander actually uses.
    pub use_frac: f64,
}

impl Default for MatteDrift {
    fn default() -> Self {
        Self {
            rate_x: 0.11,
            rate_y: 0.13,
            use_frac: 0.8,
        }
    }
}

/// A time-windowed tone adjustment over the whole composition.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ColorSpan {
    pub start_s: f64,
    pub end_s: f64,
    /// -1 (cool) .. 1 (warm).
    #[serde(default)]
    pub temperature: f64,
    /// Multiplier, 1 = neutral.
    #[serde(default = "one")]
    pub saturation: f64,
    /// -1 .. 1, 0 = neutral.
    #[serde(default)]
    pub brightness: f64,
}

fn one() -> f64 {
    1.0
}

// ── Pure geometry / graph builders ───────────────────────────────────────────

/// ffmpeg input indices for [`build_composition_filter`]: layers occupy
/// `0..layers.len()`, then one rasterized matte PNG per matted layer, then the
/// outro still and the audio.
#[derive(Debug, Clone)]
pub struct FilterLayout {
    /// Parallel to `doc.layers`: the input index of each layer's matte PNG.
    pub matte_inputs: Vec<Option<usize>>,
    pub outro: usize,
    pub audio: usize,
}

impl FilterLayout {
    /// The canonical layout for a document.
    #[must_use]
    pub fn for_doc(doc: &CompositionDoc) -> Self {
        let mut next = doc.layers.len();
        let matte_inputs = doc
            .layers
            .iter()
            .map(|l| {
                l.matte.is_some().then(|| {
                    let i = next;
                    next += 1;
                    i
                })
            })
            .collect();
        Self {
            matte_inputs,
            outro: next,
            audio: next + 1,
        }
    }
}

/// Format a float for a filtergraph (fixed precision, no locale surprises).
fn f(v: f64) -> String {
    format!("{v:.4}")
}

/// Even-rounded pixel dimension (safe for scalers and x264 alike).
fn even(v: f64) -> u32 {
    let n = v.round().max(2.0) as u32;
    n & !1
}

/// The frame a layer is scaled to: the largest keyframed rect (even-rounded),
/// or the canvas when the layer has no placement.
fn layer_frame(layer: &Layer, w: u32, h: u32) -> (u32, u32) {
    let fw = layer
        .placement
        .iter()
        .map(|k| k.w)
        .max()
        .unwrap_or(w)
        .max(2);
    let fh = layer
        .placement
        .iter()
        .map(|k| k.h)
        .max()
        .unwrap_or(h)
        .max(2);
    (fw & !1, fh & !1)
}

/// A step function of `t` as an overlay position: one keyframe is a bare
/// number (byte-stable with static graphs); several nest `if(lt(t,..))`
/// right-to-left. Values are the CENTERED origin for the layer's scaled frame.
fn step_expr(keys: &[(f64, i64)]) -> String {
    match keys {
        [] => "0".to_string(),
        [only] => only.1.to_string(),
        _ => {
            let mut acc = keys[keys.len() - 1].1.to_string();
            for pair in keys.windows(2).rev() {
                acc = format!("if(lt(t,{}),{},{})", f(pair[1].0), pair[0].1, acc);
            }
            // Quoted so the expression's commas don't read as filter
            // separators inside the graph.
            format!("'{acc}'")
        }
    }
}

/// The overlay position expressions for a layer scaled to `(fw, fh)`:
/// per-keyframe origins, centering the scaled frame on each keyframe's rect.
fn placement_exprs(layer: &Layer, fw: u32, fh: u32) -> (String, String) {
    if layer.placement.is_empty() {
        return ("0".to_string(), "0".to_string());
    }
    let xs: Vec<(f64, i64)> = layer
        .placement
        .iter()
        .map(|k| (k.at_s, k.x + (i64::from(k.w) - i64::from(fw)) / 2))
        .collect();
    let ys: Vec<(f64, i64)> = layer
        .placement
        .iter()
        .map(|k| (k.at_s, k.y + (i64::from(k.h) - i64::from(fh)) / 2))
        .collect();
    (step_expr(&xs), step_expr(&ys))
}

/// Build the complete `-filter_complex` for a composition render. Pure — unit
/// tested; `render_composition` feeds it real inputs.
pub fn build_composition_filter(
    doc: &CompositionDoc,
    layout: &FilterLayout,
) -> Result<String, MediaGatewayError> {
    if doc.version != 3 {
        return Err(MediaGatewayError::InvalidRequest(format!(
            "composition document version {} is not supported (this renderer \
             takes version 3; a version-2 progression manifest predates the \
             generic composition document — re-run `av compose`)",
            doc.version
        )));
    }
    if doc.layers.is_empty() {
        return Err(MediaGatewayError::InvalidRequest(
            "composition document has no layers".to_string(),
        ));
    }
    if layout.matte_inputs.len() != doc.layers.len() {
        return Err(MediaGatewayError::InvalidRequest(
            "filter layout must be parallel to layers".to_string(),
        ));
    }
    let (w, h) = (doc.width, doc.height);
    let fps = f64::from(doc.fps_num) / f64::from(doc.fps_den);
    if !(fps.is_finite() && fps > 0.0) {
        return Err(MediaGatewayError::InvalidRequest(
            "composition fps must be positive".to_string(),
        ));
    }
    let cut = doc.outro.at_s;
    let dur = doc.duration_seconds;
    if !(cut.is_finite() && dur.is_finite() && cut > 0.0 && dur > cut) {
        return Err(MediaGatewayError::InvalidRequest(format!(
            "composition needs 0 < outro.at_s ({cut}) < duration_seconds ({dur})"
        )));
    }
    let frame = 1.0 / fps;

    let mut g: Vec<String> = Vec::new();

    // Base plane.
    g.push(format!(
        "color=c=black:s={w}x{h}:r={}:d={}[base0]",
        f(fps),
        f(cut)
    ));

    let mut base_idx = 0usize;
    for (i, layer) in doc.layers.iter().enumerate() {
        let (fw, fh) = layer_frame(layer, w, h);
        let fit = format!(
            "fps={},scale={fw}:{fh}:force_original_aspect_ratio=increase,crop={fw}:{fh}",
            f(fps)
        );

        if layer.segments.is_empty() {
            // Continuous playback, trimmed to the cut.
            g.push(format!(
                "[{i}:v]fps={},scale={fw}:{fh}:force_original_aspect_ratio=increase,\
                 crop={fw}:{fh},trim=duration={},setpts=PTS-STARTPTS[lay{i}]",
                f(fps),
                f(cut)
            ));
        } else {
            // Time-remap: split the source per segment, trim+hold each, concat.
            // Holds are the gaps to the next segment's onset (or the cut).
            let first_at = layer.segments.first().map_or(0.0, |s| s.at_s);
            let pre = if first_at > 1.5 * frame {
                Some(first_at)
            } else {
                None
            };
            let n_segments = layer.segments.len() + usize::from(pre.is_some());

            g.push(format!(
                "[{i}:v]split={n_segments}{}",
                (0..n_segments)
                    .map(|k| format!("[p{i}s{k}]"))
                    .collect::<String>()
            ));

            let mut seg_labels: Vec<String> = Vec::new();
            let mut src = 0usize;
            if let Some(hold) = pre {
                // One frame of the source head, cloned until the first onset.
                g.push(format!(
                    "[p{i}s{src}]trim=end_frame=1,setpts=PTS-STARTPTS,{fit},\
                     tpad=stop_mode=clone:stop_duration={}[p{i}pre]",
                    f((hold - frame).max(0.0))
                ));
                seg_labels.push(format!("[p{i}pre]"));
                src += 1;
            }
            for (k, seg) in layer.segments.iter().enumerate() {
                let next_at = layer
                    .segments
                    .get(k + 1)
                    .map_or(cut, |s| s.at_s)
                    .max(seg.at_s + seg.duration_s);
                let hold = (next_at - seg.at_s - seg.duration_s).max(0.0);
                g.push(format!(
                    "[p{i}s{src}]trim=start={}:duration={},setpts=PTS-STARTPTS,{fit},\
                     tpad=stop_mode=clone:stop_duration={}[p{i}k{k}]",
                    f(seg.source_in_s),
                    f(seg.duration_s),
                    f(hold)
                ));
                seg_labels.push(format!("[p{i}k{k}]"));
                src += 1;
            }
            g.push(format!(
                "{}concat=n={}:v=1:a=0[lay{i}]",
                seg_labels.concat(),
                seg_labels.len()
            ));
        }

        // Matte: luma from the rasterized asset, optional drift + invert, cut
        // into the layer's alpha.
        let mut layer_label = format!("[lay{i}]");
        if let (Some(matte), Some(m_idx)) = (&layer.matte, layout.matte_inputs[i]) {
            let invert = if matte.invert { ",negate" } else { "" };
            if let Some(drift) = &matte.drift {
                let mw = even(f64::from(fw) * MATTE_OVERSIZE);
                let mh = even(f64::from(fh) * MATTE_OVERSIZE);
                let ampx = (f64::from(mw - fw) / 2.0 * drift.use_frac).floor();
                let ampy = (f64::from(mh - fh) / 2.0 * drift.use_frac).floor();
                g.push(format!(
                    "[{m_idx}:v]format=gray,crop={fw}:{fh}:x='(iw-ow)/2+{}*sin({}*t)':y='(ih-oh)/2+{}*cos({}*t)'{invert}[mat{i}]",
                    f(ampx),
                    f(drift.rate_x),
                    f(ampy),
                    f(drift.rate_y)
                ));
            } else {
                g.push(format!("[{m_idx}:v]format=gray{invert}[mat{i}]"));
            }
            g.push(format!("{layer_label}[mat{i}]alphamerge[lm{i}]"));
            layer_label = format!("[lm{i}]");
        }

        let (x_expr, y_expr) = placement_exprs(layer, fw, fh);
        g.push(format!(
            "[base{base_idx}]{layer_label}overlay=x={x_expr}:y={y_expr}:eof_action=pass[base{}]",
            base_idx + 1
        ));
        base_idx += 1;
    }

    // Color-span chain (skipped entirely when unauthored).
    let mut comp = format!("[base{base_idx}]");
    if !doc.color_spans.is_empty() {
        let mut chain: Vec<String> = Vec::new();
        for span in &doc.color_spans {
            let enable = format!("enable='between(t,{},{})'", f(span.start_s), f(span.end_s));
            let temp = span.temperature.clamp(-1.0, 1.0);
            if temp.abs() > 1e-6 {
                chain.push(format!(
                    "colorbalance=rm={}:bm={}:{enable}",
                    f(0.3 * temp),
                    f(-0.3 * temp)
                ));
            }
            let sat = span.saturation.clamp(0.0, 3.0);
            let bright = span.brightness.clamp(-1.0, 1.0);
            if (sat - 1.0).abs() > 1e-6 || bright.abs() > 1e-6 {
                chain.push(format!(
                    "eq=saturation={}:brightness={}:{enable}",
                    f(sat),
                    f(0.3 * bright)
                ));
            }
        }
        if !chain.is_empty() {
            g.push(format!("{comp}{}[graded]", chain.join(",")));
            comp = "[graded]".to_string();
        }
    }

    // Main segment (0..cut) + outro hold (cut..duration), hard cut between.
    g.push(format!(
        "{comp}trim=duration={},setpts=PTS-STARTPTS,setsar=1[mainv]",
        f(cut)
    ));
    let hold = dur - cut;
    g.push(format!(
        "[{}:v]scale={w}:{h}:force_original_aspect_ratio=decrease,\
         pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1,fps={},\
         trim=duration={},setpts=PTS-STARTPTS[revv]",
        layout.outro,
        f(fps),
        f(hold)
    ));
    g.push("[mainv][revv]concat=n=2:v=1:a=0[outv]".to_string());

    // The audio track — trimmed to the output length, gain honored.
    g.push(format!(
        "[{}:a]atrim=duration={},asetpts=PTS-STARTPTS,volume={}[outa]",
        layout.audio,
        f(dur),
        f(doc.audio.gain.clamp(0.0, 4.0))
    ));

    Ok(g.join(";"))
}

// ── Orchestration (I/O) ──────────────────────────────────────────────────────

/// Read an asset's bytes into a scratch file.
async fn stage_asset(
    pool: &DbPool,
    asset_id: Uuid,
    path: &Path,
    what: &str,
) -> Result<(), MediaGatewayError> {
    let bytes = read_asset_content(pool, asset_id).await.map_err(|_| {
        MediaGatewayError::NotFound(format!(
            "{what} asset {asset_id} not found (wrong id or never finalized)"
        ))
    })?;
    if bytes.is_empty() {
        return Err(MediaGatewayError::NotFound(format!(
            "{what} asset {asset_id} has no content (never finalized?)"
        )));
    }
    tokio::fs::write(path, bytes)
        .await
        .map_err(|e| MediaGatewayError::MediaTool(format!("write {what} to scratch: {e}")))?;
    Ok(())
}

/// Render a composition document to the final mp4 asset. Returns
/// `(asset_id, content_length, duration_seconds)`.
pub async fn render_composition(
    pool: &Arc<DbPool>,
    scratch_root: &Path,
    doc: &CompositionDoc,
) -> Result<(Uuid, i64, f64), MediaGatewayError> {
    if doc.duration_seconds > MAX_RENDER_SECONDS {
        return Err(MediaGatewayError::InvalidRequest(format!(
            "render duration {:.1}s exceeds the {MAX_RENDER_SECONDS:.0}s cap",
            doc.duration_seconds
        )));
    }
    // Even output dims for yuv420p/x264.
    let mut doc = doc.clone();
    doc.width = even(f64::from(doc.width));
    doc.height = even(f64::from(doc.height));

    let dir = scratch_root.join(format!("render-{}", Uuid::now_v7()));
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| MediaGatewayError::MediaTool(format!("create scratch dir: {e}")))?;

    let result = render_composition_in(pool, &dir, &doc).await;

    // Best-effort scratch cleanup either way (the full render tree is large).
    if let Err(e) = tokio::fs::remove_dir_all(&dir).await {
        log::warn!("failed to clean render scratch {}: {e}", dir.display());
    }
    result
}

async fn render_composition_in(
    pool: &Arc<DbPool>,
    dir: &Path,
    doc: &CompositionDoc,
) -> Result<(Uuid, i64, f64), MediaGatewayError> {
    // 1. Stage layer sources.
    let mut layer_paths: Vec<PathBuf> = Vec::new();
    for (i, layer) in doc.layers.iter().enumerate() {
        let path = dir.join(format!("layer-{i}.mp4"));
        stage_asset(pool, layer.asset_id, &path, &format!("layer[{i}]")).await?;
        layer_paths.push(path);
    }

    // 2. Mattes: rasterize each matted layer's asset at its (possibly
    //    oversized, for drift) frame size.
    let mut matte_paths: Vec<Option<PathBuf>> = Vec::new();
    for (i, layer) in doc.layers.iter().enumerate() {
        let Some(matte) = &layer.matte else {
            matte_paths.push(None);
            continue;
        };
        let (fw, fh) = layer_frame(layer, doc.width, doc.height);
        let (mw, mh) = if matte.drift.is_some() {
            (
                even(f64::from(fw) * MATTE_OVERSIZE),
                even(f64::from(fh) * MATTE_OVERSIZE),
            )
        } else {
            (fw, fh)
        };
        let bytes = read_asset_content(pool, matte.asset_id)
            .await
            .map_err(|_| {
                MediaGatewayError::NotFound(format!(
                    "matte asset {} not found (wrong id or never finalized)",
                    matte.asset_id
                ))
            })?;
        let png = rasterize_mask(&bytes, mw, mh)?;
        let path = dir.join(format!("matte-{i}.png"));
        tokio::fs::write(&path, png)
            .await
            .map_err(|e| MediaGatewayError::MediaTool(format!("write matte png: {e}")))?;
        matte_paths.push(Some(path));
    }

    let outro_path = dir.join("outro.png");
    stage_asset(pool, doc.outro.asset_id, &outro_path, "outro still").await?;
    let audio_path = dir.join("audio.src");
    stage_asset(pool, doc.audio.asset_id, &audio_path, "audio").await?;

    // 3. Build the graph + args.
    let layout = FilterLayout::for_doc(doc);
    let filter = build_composition_filter(doc, &layout)?;
    let filter_path = dir.join("filter.txt");
    tokio::fs::write(&filter_path, &filter)
        .await
        .map_err(|e| MediaGatewayError::MediaTool(format!("write filter script: {e}")))?;

    let fps = f64::from(doc.fps_num) / f64::from(doc.fps_den);
    let cut_s = format!("{:.4}", doc.outro.at_s);
    let hold_s = format!("{:.4}", doc.duration_seconds - doc.outro.at_s);
    let dur_s = format!("{:.4}", doc.duration_seconds);
    let fps_s = format!("{fps:.4}");
    let out_path = dir.join("out.mp4");

    let mut args: Vec<String> = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
    ];
    for (layer, path) in doc.layers.iter().zip(&layer_paths) {
        if layer.loop_source {
            args.extend(["-stream_loop".into(), "-1".into()]);
        }
        args.extend(["-i".into(), path.display().to_string()]);
    }
    for path in matte_paths.iter().flatten() {
        args.extend([
            "-loop".into(),
            "1".into(),
            "-framerate".into(),
            fps_s.clone(),
            "-t".into(),
            cut_s.clone(),
            "-i".into(),
            path.display().to_string(),
        ]);
    }
    args.extend([
        "-loop".into(),
        "1".into(),
        "-framerate".into(),
        fps_s,
        "-t".into(),
        hold_s,
        "-i".into(),
        outro_path.display().to_string(),
    ]);
    args.extend(["-i".into(), audio_path.display().to_string()]);
    args.extend([
        "-filter_complex_script".into(),
        filter_path.display().to_string(),
        "-map".into(),
        "[outv]".into(),
        "-map".into(),
        "[outa]".into(),
        "-c:v".into(),
        "libx264".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-preset".into(),
        "veryfast".into(),
        "-c:a".into(),
        "aac".into(),
        "-ar".into(),
        "48000".into(),
        "-ac".into(),
        "2".into(),
        "-movflags".into(),
        "+faststart".into(),
        "-t".into(),
        dur_s,
        out_path.display().to_string(),
    ]);

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    super::media::run_ffmpeg(&arg_refs).await?;

    // 4. Persist.
    let produced = probe_duration(&out_path).await.unwrap_or(0.0);
    let bytes = tokio::fs::read(&out_path)
        .await
        .map_err(|e| MediaGatewayError::MediaTool(format!("read rendered mp4: {e}")))?;
    if bytes.is_empty() {
        return Err(MediaGatewayError::MediaTool(
            "ffmpeg produced an empty render".to_string(),
        ));
    }
    let (asset_id, content_length) =
        persist_media_asset(pool, &bytes, "video", "video/mp4").await?;

    log::info!(
        "composition render complete: layers={}, cut={:.2}s, duration={:.2}s (probed {produced:.2}s), asset={asset_id}, bytes={content_length}",
        doc.layers.len(),
        doc.outro.at_s,
        doc.duration_seconds
    );
    Ok((asset_id, content_length, doc.duration_seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical two-layer doc — what the quilt compiler emits for the old
    /// two-pattern fixture: layer rects are padded dot rects on a 1280x720
    /// canvas (dot alpha: cx .25, r .1 → 294x166 at (173,277); dot beta:
    /// cx .75, r .15 → 442x248 at (739,236)).
    fn doc() -> CompositionDoc {
        CompositionDoc {
            version: 3,
            name: "Quilt".to_string(),
            fps_num: 24,
            fps_den: 1,
            width: 1280,
            height: 720,
            duration_seconds: 60.0,
            audio: AudioSource {
                asset_id: Uuid::from_u128(10),
                gain: 1.0,
            },
            outro: OutroStill {
                at_s: 50.0,
                asset_id: Uuid::from_u128(12),
            },
            color_spans: vec![ColorSpan {
                start_s: 10.0,
                end_s: 20.0,
                temperature: 0.5,
                saturation: 1.2,
                brightness: 0.1,
            }],
            layers: vec![
                Layer {
                    name: "pads".to_string(),
                    asset_id: Uuid::from_u128(1),
                    loop_source: false,
                    segments: vec![
                        Segment {
                            at_s: 2.0,
                            source_in_s: 0.0,
                            duration_s: 4.0,
                        },
                        Segment {
                            at_s: 30.0,
                            source_in_s: 4.0,
                            duration_s: 4.0,
                        },
                    ],
                    placement: vec![PlacementKey {
                        at_s: 0.0,
                        x: 173,
                        y: 277,
                        w: 294,
                        h: 166,
                    }],
                    matte: None,
                },
                Layer {
                    name: "keys".to_string(),
                    asset_id: Uuid::from_u128(2),
                    loop_source: false,
                    segments: vec![Segment {
                        at_s: 50.0,
                        source_in_s: 0.0,
                        duration_s: 12.0,
                    }],
                    placement: vec![PlacementKey {
                        at_s: 0.0,
                        x: 739,
                        y: 236,
                        w: 442,
                        h: 248,
                    }],
                    matte: None,
                },
                Layer {
                    name: "foreground".to_string(),
                    asset_id: Uuid::from_u128(13),
                    loop_source: true,
                    segments: vec![],
                    placement: vec![],
                    matte: Some(Matte {
                        asset_id: Uuid::from_u128(11),
                        invert: true,
                        drift: Some(MatteDrift::default()),
                    }),
                },
            ],
        }
    }

    #[test]
    fn layout_indices_are_contiguous() {
        let d = doc();
        let l = FilterLayout::for_doc(&d);
        // 3 layers, 1 matte: matte input 3, outro 4, audio 5.
        assert_eq!(l.matte_inputs, vec![None, None, Some(3)]);
        assert_eq!(l.outro, 4);
        assert_eq!(l.audio, 5);

        let mut no_fg = d;
        no_fg.layers.truncate(2);
        let l = FilterLayout::for_doc(&no_fg);
        assert_eq!(l.matte_inputs, vec![None, None]);
        assert_eq!(l.outro, 2);
        assert_eq!(l.audio, 3);
    }

    #[test]
    fn filter_contains_every_structural_stage() {
        let d = doc();
        let l = FilterLayout::for_doc(&d);
        let filter = build_composition_filter(&d, &l).unwrap();

        // Base plane at output size, cut-long.
        assert!(filter.contains("color=c=black:s=1280x720"), "{filter}");
        // Layer 0: pre-hold (first onset 2.0s > a frame), two segments, concat.
        assert!(filter.contains("[p0pre]"), "pre-hold missing: {filter}");
        assert!(filter.contains("trim=start=0.0000:duration=4.0000"));
        assert!(filter.contains("trim=start=4.0000:duration=4.0000"));
        // Freeze hold: segment 0 ends at 6.0, next onset 30.0 → 24s clone.
        assert!(
            filter.contains("tpad=stop_mode=clone:stop_duration=24.0000"),
            "freeze hold missing: {filter}"
        );
        // Static placement renders bare numeric overlay coords.
        assert!(filter.contains("overlay=x=173:y=277"), "{filter}");
        assert!(filter.contains("overlay=x=739:y=236"), "{filter}");
        // Matted layer: continuous (no segments), inverted drifting matte.
        assert!(filter.contains("alphamerge"), "{filter}");
        assert!(filter.contains("negate"), "{filter}");
        assert!(filter.contains("sin(0.1100*t)"), "drift missing: {filter}");
        assert!(filter.contains("overlay=x=0:y=0"), "{filter}");
        // Color span windows.
        assert!(
            filter.contains("between(t,10.0000,20.0000)"),
            "color window missing: {filter}"
        );
        assert!(filter.contains("colorbalance="), "{filter}");
        assert!(filter.contains("eq=saturation=1.2000"), "{filter}");
        // Hard cut: main trimmed to 50s, outro concat'd after.
        assert!(filter.contains("trim=duration=50.0000"), "{filter}");
        assert!(filter.contains("[mainv][revv]concat=n=2:v=1:a=0[outv]"));
        // Audio mapped from the right input with gain.
        assert!(filter.contains("[5:a]atrim=duration=60.0000"), "{filter}");
    }

    #[test]
    fn multi_keyframe_placement_renders_a_piecewise_position() {
        let mut d = doc();
        // Layer 0 trades rects at 20s and 35s (the "patches trade windows"
        // move a strategy compiles from section boundaries).
        d.layers[0].placement = vec![
            PlacementKey {
                at_s: 0.0,
                x: 173,
                y: 277,
                w: 294,
                h: 166,
            },
            PlacementKey {
                at_s: 20.0,
                x: 739,
                y: 236,
                w: 442,
                h: 248,
            },
            PlacementKey {
                at_s: 35.0,
                x: 173,
                y: 277,
                w: 294,
                h: 166,
            },
        ];
        let l = FilterLayout::for_doc(&d);
        let filter = build_composition_filter(&d, &l).unwrap();

        // Scaled once to the largest visited rect (442x248), so the smaller
        // keyframes center it: x = 173 + (294-442)/2 = 99, y = 277 + (166-248)/2 = 236.
        assert!(
            filter.contains("overlay=x='if(lt(t,20.0000),99,if(lt(t,35.0000),739,99))'"),
            "piecewise x missing: {filter}"
        );
        assert!(
            filter.contains(":y='if(lt(t,20.0000),236,if(lt(t,35.0000),236,236))'"),
            "piecewise y missing: {filter}"
        );
        // The layer is scaled to the max rect exactly once.
        assert!(
            filter.contains("scale=442:248:force_original_aspect_ratio=increase,crop=442:248"),
            "max-rect scale missing: {filter}"
        );
    }

    #[test]
    fn single_keyframe_stays_static_and_unquoted() {
        // The no-moves case must not regress into expression syntax: bare
        // numbers keep the graph identical in shape to the pre-keyframe
        // renderer.
        let d = doc();
        let l = FilterLayout::for_doc(&d);
        let filter = build_composition_filter(&d, &l).unwrap();
        assert!(!filter.contains("if(lt(t,"), "{filter}");
    }

    #[test]
    fn filter_is_deterministic() {
        let d = doc();
        let l = FilterLayout::for_doc(&d);
        assert_eq!(
            build_composition_filter(&d, &l).unwrap(),
            build_composition_filter(&d, &l).unwrap()
        );
    }

    #[test]
    fn filter_without_matte_or_color_still_builds() {
        let mut d = doc();
        d.layers.truncate(2);
        d.color_spans.clear();
        let l = FilterLayout::for_doc(&d);
        let filter = build_composition_filter(&d, &l).unwrap();
        assert!(!filter.contains("alphamerge"));
        assert!(!filter.contains("colorbalance"));
        assert!(filter.contains("[mainv][revv]concat"));
        // Outro/audio indices follow the shrunk input list.
        assert!(filter.contains("[2:v]scale=1280:720"), "{filter}");
        assert!(filter.contains("[3:a]atrim="), "{filter}");
    }

    #[test]
    fn filter_rejects_bad_shapes() {
        let d = doc();
        let l = FilterLayout::for_doc(&d);

        let mut no_layers = d.clone();
        no_layers.layers.clear();
        assert!(build_composition_filter(&no_layers, &FilterLayout::for_doc(&no_layers)).is_err());

        let mut bad_cut = d.clone();
        bad_cut.outro.at_s = bad_cut.duration_seconds + 1.0;
        assert!(build_composition_filter(&bad_cut, &l).is_err());

        // A retired version-2 progression manifest is rejected with guidance.
        let mut v2 = d.clone();
        v2.version = 2;
        let err = build_composition_filter(&v2, &l).unwrap_err();
        assert!(format!("{err:?}").contains("re-run"), "{err:?}");
    }

    #[test]
    fn doc_wire_roundtrip_matches_oximedia_shape() {
        // The exact JSON conduit-oximedia's compile_composition emits.
        let json = serde_json::json!({
            "version": 3,
            "name": "Quilt",
            "fps_num": 24, "fps_den": 1,
            "width": 1280, "height": 720,
            "duration_seconds": 60.0,
            "audio": { "asset_id": Uuid::from_u128(10), "gain": 1.0 },
            "outro": { "at_s": 50.0, "asset_id": Uuid::from_u128(12) },
            "color_spans": [
                { "start_s": 10.0, "end_s": 20.0,
                  "temperature": 0.5, "saturation": 1.2, "brightness": 0.1 }
            ],
            "layers": [
                { "name": "pads", "asset_id": Uuid::from_u128(1),
                  "segments": [ { "at_s": 2.0, "source_in_s": 0.0, "duration_s": 4.0 } ],
                  "placement": [
                      { "at_s": 0.0, "x": 173, "y": 277, "w": 294, "h": 166 },
                      { "at_s": 20.0, "x": 739, "y": 236, "w": 442, "h": 248 }
                  ] },
                { "name": "foreground", "asset_id": Uuid::from_u128(13),
                  "loop_source": true,
                  "matte": { "asset_id": Uuid::from_u128(11), "invert": true,
                             "drift": { "rate_x": 0.11, "rate_y": 0.13, "use_frac": 0.8 } } }
            ]
        });
        let d: CompositionDoc = serde_json::from_value(json).unwrap();
        assert_eq!(d.version, 3);
        assert_eq!(d.layers.len(), 2);
        assert_eq!(d.layers[0].placement.len(), 2);
        assert!(d.layers[1].loop_source);
        assert!(d.layers[1].matte.unwrap().invert);
        assert_eq!(d.color_spans.len(), 1);
    }
}
