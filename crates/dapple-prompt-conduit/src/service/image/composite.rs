//! Deterministic image compositing + color-grade for the generate-only pipeline.
//!
//! This is the "superior tooling" half of the packed-panorama approach: the model
//! only ever produces net-new pixels via `generate()`; everything here is pure,
//! local, deterministic image processing that PRESERVES the pixels it is given.
//! No network, no model, no round-tripping an existing/composed image through a
//! generative service (the inviolability principle — see `service/image/mod.rs`).
//!
//! Pipeline per block: decode → resize to its target cell → optionally
//! color-match it to the background (Reinhard per-channel mean/σ transfer) →
//! optionally feather its edges and drop a soft contact shadow → alpha-composite
//! it onto the background canvas. Independently generated pieces are matched into
//! one palette here; what this layer cannot do (unify perspective / drawing-hand)
//! is the job of generation-time consistency, not a generative cleanup pass.

use super::layout::{LayoutParams, LayoutStrategy, MeasuredPiece, Placement};
use crate::error::ConduitError;
use zune_core::bit_depth::BitDepth;
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_image::codecs::ImageFormat;
use zune_image::image::Image;
use zune_image::traits::OperationsTrait;
use zune_imageprocs::composite::{Composite, CompositeMethod};
use zune_imageprocs::resize::{Resize, ResizeMethod};

/// One generated block to place on the background canvas, with its target cell.
#[derive(Debug, Clone)]
pub struct BlockPlacement {
    /// Encoded PNG bytes of the generated block.
    pub png_bytes: Vec<u8>,
    /// Target top-left x on the background, in pixels.
    pub x: u32,
    /// Target top-left y on the background, in pixels.
    pub y: u32,
    /// Target cell width; the block is resized to this.
    pub w: u32,
    /// Target cell height; the block is resized to this.
    pub h: u32,
}

/// Deterministic knobs for a composite pass (tuned in A′, never by a model).
#[derive(Debug, Clone, Copy)]
pub struct CompositeOpts {
    /// Reinhard per-channel mean/σ match of each block to the background.
    pub color_match: bool,
    /// Border feather radius in pixels (alpha ramp); 0 disables.
    pub feather: u32,
    /// Drop a soft procedural contact shadow under each block.
    pub shadow: bool,
}

impl Default for CompositeOpts {
    fn default() -> Self {
        Self {
            color_match: true,
            feather: 0,
            shadow: false,
        }
    }
}

/// Soft-shadow constants (deterministic; revisit in A′ tuning).
const SHADOW_ALPHA: u8 = 110;
const SHADOW_OFFSET: u32 = 6;
const SHADOW_FEATHER: u32 = 10;

/// Defringe constants for cleaning gpt-image transparent-cutout edges.
/// gpt-image mattes cutouts over a near-white backing, so the semi-transparent
/// edge pixels carry baked-in white that straight-alpha compositing paints as a
/// halo. We erode the outermost ring (removes the ring whatever its colour),
/// drop the faintest residual alpha, and invert the white matte on what remains.
const DEFRINGE_ERODE: usize = 2;
const DEFRINGE_ALPHA_FLOOR: u8 = 24;

fn img_err<E: std::fmt::Debug>(ctx: &str) -> impl Fn(E) -> ConduitError + '_ {
    move |e| ConduitError::ImageError(format!("composite: {ctx}: {e:?}"))
}

/// Decode PNG bytes into an 8-bit RGBA `Image` (the canonical working format so
/// blocks and background share depth + colorspace, which `Composite` requires).
/// `what` labels the image (`"background"` / `"block[2]"`) and the byte count is
/// folded into the error so a decode failure pinpoints the offending input
/// (empty vs corrupt, which piece) without needing the conduit logs.
fn decode_rgba8(bytes: &[u8], what: &str) -> Result<Image, ConduitError> {
    let n = bytes.len();
    let ctx = |stage: &str| format!("{what}: {stage} ({n} bytes)");
    let mut img = Image::read(ZCursor::new(bytes), DecoderOptions::default())
        .map_err(img_err(&ctx("decode")))?;
    img.convert_depth(BitDepth::Eight)
        .map_err(img_err(&ctx("to-depth8")))?;
    img.convert_color(ColorSpace::RGBA)
        .map_err(img_err(&ctx("to-rgba")))?;
    Ok(img)
}

pub(super) fn encode_png(img: &Image) -> Result<Vec<u8>, ConduitError> {
    img.write_to_vec(ImageFormat::PNG)
        .map_err(img_err("encode"))
}

/// Per-RGB-channel `(mean, std)` over an RGBA image, counting only **visible**
/// pixels (alpha > 0). A transparent-cutout piece is mostly empty backing; if
/// that backing's RGB (often white) were folded into the stats, the Reinhard
/// match would grade the drawn content against the wrong palette. The opaque
/// background canvas has alpha 255 everywhere, so its stats are unchanged.
fn rgb_stats(img: &Image) -> Result<[(f32, f32); 3], ConduitError> {
    let channels = img.channels_ref(false); // R, G, B, A
    let alpha: &[u8] = channels
        .get(3)
        .ok_or_else(|| ConduitError::ImageError("composite: stats: missing alpha".into()))?
        .reinterpret_as()
        .map_err(img_err("stats-alpha"))?;
    let mut out = [(0.0f32, 0.0f32); 3];
    for (i, ch) in channels.iter().take(3).enumerate() {
        let data: &[u8] = ch.reinterpret_as().map_err(img_err("stats-channel"))?;
        if data.is_empty() {
            continue;
        }
        let visible = || {
            data.iter()
                .zip(alpha)
                .filter(|(_, &a)| a > 0)
                .map(|(&v, _)| v)
        };
        let n = visible().count() as f32;
        if n == 0.0 {
            continue;
        }
        let mean = visible().map(|v| v as f32).sum::<f32>() / n;
        let var = visible().map(|v| (v as f32 - mean).powi(2)).sum::<f32>() / n;
        out[i] = (mean, var.sqrt());
    }
    Ok(out)
}

/// Reinhard per-channel transfer: shift each RGB channel of `block` so its
/// mean/σ matches `target` (the background). Alpha is left untouched.
fn color_match(block: &mut Image, target: &[(f32, f32); 3]) -> Result<(), ConduitError> {
    let src = rgb_stats(block)?;
    block
        .modify_pixels_mut::<u8, _>(|_y, _x, px| {
            for c in 0..3 {
                let (s_mean, s_std) = src[c];
                let (t_mean, t_std) = target[c];
                let z = (*px[c] as f32 - s_mean) / s_std.max(1e-3);
                let v = (z * t_std + t_mean).clamp(0.0, 255.0);
                *px[c] = v as u8;
            }
        })
        .map_err(img_err("color-match"))?;
    Ok(())
}

/// Ramp the alpha of an RGBA image toward 0 within `radius` px of the border, so
/// the composite blends instead of showing a hard cut edge.
fn feather_alpha(img: &mut Image, radius: u32) -> Result<(), ConduitError> {
    if radius == 0 {
        return Ok(());
    }
    let (w, h) = img.dimensions();
    let (w, h) = (w as i64, h as i64);
    let f = radius as f32;
    img.modify_pixels_mut::<u8, _>(|y, x, px| {
        let x = x as i64;
        let y = y as i64;
        let dx = x.min(w - 1 - x) as f32;
        let dy = y.min(h - 1 - y) as f32;
        let d = dx.min(dy);
        if d < f {
            let scaled = *px[3] as f32 * (d / f);
            *px[3] = scaled as u8;
        }
    })
    .map_err(img_err("feather"))?;
    Ok(())
}

/// Extract a copy of the alpha channel of an RGBA image.
fn alpha_channel(img: &Image) -> Result<Vec<u8>, ConduitError> {
    let channels = img.channels_ref(false);
    let a: &[u8] = channels
        .get(3)
        .ok_or_else(|| ConduitError::ImageError("composite: missing alpha channel".into()))?
        .reinterpret_as()
        .map_err(img_err("alpha-read"))?;
    Ok(a.to_vec())
}

/// Overwrite an RGBA image's alpha channel from a row-major buffer.
fn set_alpha(img: &mut Image, alpha: &[u8]) -> Result<(), ConduitError> {
    let w = img.dimensions().0;
    img.modify_pixels_mut::<u8, _>(|y, x, px| {
        *px[3] = alpha[y * w + x];
    })
    .map_err(img_err("alpha-write"))?;
    Ok(())
}

/// Separable box blur (mean filter) over a single-channel buffer, border-clamped.
fn box_blur(src: &[u8], w: usize, h: usize, radius: usize) -> Vec<u8> {
    if radius == 0 || w == 0 || h == 0 {
        return src.to_vec();
    }
    let mut tmp = vec![0u8; w * h];
    for y in 0..h {
        let base = y * w;
        for x in 0..w {
            let lo = x.saturating_sub(radius);
            let hi = (x + radius).min(w - 1);
            let slice = &src[base + lo..=base + hi];
            let sum: u32 = slice.iter().map(|&v| v as u32).sum();
            tmp[base + x] = (sum / slice.len() as u32) as u8;
        }
    }
    let mut out = vec![0u8; w * h];
    for x in 0..w {
        for y in 0..h {
            let lo = y.saturating_sub(radius);
            let hi = (y + radius).min(h - 1);
            let sum: u32 = (lo..=hi).map(|yy| tmp[yy * w + x] as u32).sum();
            out[y * w + x] = (sum / (hi - lo + 1) as u32) as u8;
        }
    }
    out
}

/// Separable min filter (morphological erosion) over a single-channel buffer.
fn min_filter(src: &[u8], w: usize, h: usize, radius: usize) -> Vec<u8> {
    if radius == 0 || w == 0 || h == 0 {
        return src.to_vec();
    }
    let mut tmp = vec![0u8; w * h];
    for y in 0..h {
        let base = y * w;
        for x in 0..w {
            let lo = x.saturating_sub(radius);
            let hi = (x + radius).min(w - 1);
            tmp[base + x] = src[base + lo..=base + hi]
                .iter()
                .copied()
                .min()
                .unwrap_or(255);
        }
    }
    let mut out = vec![0u8; w * h];
    for x in 0..w {
        for y in 0..h {
            let lo = y.saturating_sub(radius);
            let hi = (y + radius).min(h - 1);
            out[y * w + x] = (lo..=hi).map(|yy| tmp[yy * w + x]).min().unwrap_or(255);
        }
    }
    out
}

/// Erode the alpha silhouette by `radius` px — removes the outermost ring of a
/// cutout (where gpt-image's white matte lives), regardless of its colour.
fn erode_alpha(img: &mut Image, radius: usize) -> Result<(), ConduitError> {
    if radius == 0 {
        return Ok(());
    }
    let (w, h) = img.dimensions();
    let a = alpha_channel(img)?;
    let eroded = min_filter(&a, w, h, radius);
    set_alpha(img, &eroded)
}

/// Invert the white matte baked into a cutout's semi-transparent edge pixels and
/// drop the faintest residual alpha, so the composite blends the drawn colour
/// rather than a white halo. `true_c = (matted_c - 255·(1-a)) / a`.
fn defringe_white_matte(img: &mut Image) -> Result<(), ConduitError> {
    img.modify_pixels_mut::<u8, _>(|_y, _x, px| {
        let a = *px[3];
        if a == 0 {
            return;
        }
        if a < DEFRINGE_ALPHA_FLOOR {
            *px[3] = 0;
            return;
        }
        if a < 255 {
            let af = a as f32 / 255.0;
            let unmatte = |c: u8| (((c as f32 - 255.0 * (1.0 - af)) / af).clamp(0.0, 255.0)) as u8;
            *px[0] = unmatte(*px[0]);
            *px[1] = unmatte(*px[1]);
            *px[2] = unmatte(*px[2]);
        }
    })
    .map_err(img_err("defringe"))?;
    Ok(())
}

/// Build a soft contact shadow shaped from the block's own alpha silhouette (not
/// its bounding box, which would cast a grey rectangle). Black, scaled to
/// `alpha`, then blurred for the soft contact edge.
fn make_shadow_from_alpha(block: &Image, alpha: u8) -> Result<Image, ConduitError> {
    let (w, h) = block.dimensions();
    let mut a = alpha_channel(block)?;
    for v in a.iter_mut() {
        *v = ((*v as u16 * alpha as u16) / 255) as u8;
    }
    let blurred = box_blur(&a, w, h, SHADOW_FEATHER as usize);
    let mut px = vec![0u8; w * h * 4];
    for (chunk, &av) in px.chunks_exact_mut(4).zip(blurred.iter()) {
        chunk[3] = av; // RGB stays 0 → black silhouette
    }
    Ok(Image::from_u8(&px, w, h, ColorSpace::RGBA))
}

/// Composite `blocks` onto the decoded `background_png`, returning the encoded
/// PNG plus its `(width, height)`. Deterministic end-to-end.
pub fn composite_blocks(
    background_png: &[u8],
    blocks: &[BlockPlacement],
    opts: CompositeOpts,
    canvas_size: Option<(u32, u32)>,
) -> Result<(Vec<u8>, u32, u32), ConduitError> {
    let mut canvas = decode_rgba8(background_png, "background")?;

    // Optional output-canvas upscale: redraw the base (a calm, hazy backdrop) at a larger size
    // so the crisp generated pieces composite at higher resolution. gpt-image caps the base at
    // its native size; this lifts the whole composite above that ceiling. Same Lanczos resize
    // used for blocks; block coords arrive already in this canvas's space.
    if let Some((cw, ch)) = canvas_size {
        let (bw, bh) = canvas.dimensions();
        if cw > 0 && ch > 0 && (cw, ch) != (bw as u32, bh as u32) {
            Resize::new(cw as usize, ch as usize, ResizeMethod::Lanczos3)
                .execute(&mut canvas)
                .map_err(img_err("canvas-resize"))?;
        }
    }

    let bg_stats = rgb_stats(&canvas)?;

    for (i, block) in blocks.iter().enumerate() {
        let mut img = decode_rgba8(&block.png_bytes, &format!("block[{i}]"))?;
        place_block(
            &mut canvas,
            &mut img,
            Rect {
                x: block.x,
                y: block.y,
                w: block.w,
                h: block.h,
            },
            opts,
            &bg_stats,
        )?;
    }

    // zune's `Composite::Over` leaves the destination alpha at the source
    // block's alpha in covered pixels rather than resolving to opaque over an
    // already-opaque canvas. The final spread sits on an opaque base render and
    // is always displayed opaque, so flatten any residual transparency —
    // otherwise sub-255 alpha shows a checkerboard bleed-through. RGB already
    // holds the correct blended result; only alpha needs forcing to 255.
    canvas
        .modify_pixels_mut::<u8, _>(|_y, _x, px| {
            *px[3] = 255;
        })
        .map_err(img_err("flatten-alpha"))?;

    let (w, h) = canvas.dimensions();
    let bytes = encode_png(&canvas)?;
    Ok((bytes, w as u32, h as u32))
}

/// Target rectangle (top-left + size) for a block on the canvas.
#[derive(Debug, Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// Clean a decoded cutout in place so its edges composite/export cleanly: erode
/// the outermost ring (kills gpt-image's white "sticker halo" whatever its
/// colour), invert the white matte baked into the semi-transparent edge pixels,
/// then optionally Reinhard color-match it into the background's palette. Does
/// NOT resize, feather, shadow, or composite. Shared by `place_block` (raster
/// assembly) and the SVG emitter (per-piece export) so both run the identical
/// edge-cleanup + grade, just at different resolutions and into different sinks.
pub(super) fn clean_piece(
    block: &mut Image,
    color_match_on: bool,
    bg_stats: &[(f32, f32); 3],
) -> Result<(), ConduitError> {
    erode_alpha(block, DEFRINGE_ERODE)?;
    defringe_white_matte(block)?;
    if color_match_on {
        color_match(block, bg_stats)?;
    }
    Ok(())
}

/// Place one already-decoded block onto the canvas at `rect`: resize to the cell,
/// clean the cutout edges (erode + defringe), optional Reinhard color-match,
/// optional silhouette contact shadow, optional feather, then alpha-composite.
/// Shared by `composite_blocks` (caller-positioned) and `compose_with_layout`
/// (strategy-positioned) so both paths run the identical quality pipeline.
fn place_block(
    canvas: &mut Image,
    block: &mut Image,
    rect: Rect,
    opts: CompositeOpts,
    bg_stats: &[(f32, f32); 3],
) -> Result<(), ConduitError> {
    // Resize to the target cell.
    let (cw, ch) = (rect.w.max(1) as usize, rect.h.max(1) as usize);
    let (iw, ih) = block.dimensions();
    if (iw, ih) != (cw, ch) {
        Resize::new(cw, ch, ResizeMethod::Lanczos3)
            .execute(block)
            .map_err(img_err("resize"))?;
    }

    // Clean the cutout edges + optional grade (erode → defringe → color-match).
    clean_piece(block, opts.color_match, bg_stats)?;

    // Soft contact shadow goes down first, offset under the block — shaped from
    // the cleaned silhouette so it never paints a grey rectangle.
    if opts.shadow {
        let shadow = make_shadow_from_alpha(block, SHADOW_ALPHA)?;
        let sx = (rect.x + SHADOW_OFFSET) as usize;
        let sy = (rect.y + SHADOW_OFFSET) as usize;
        Composite::new(&shadow, CompositeMethod::Over, (sx, sy))
            .execute(canvas)
            .map_err(img_err("shadow-composite"))?;
    }

    // Feather any hard rectangular frame, then place the block.
    feather_alpha(block, opts.feather)?;
    Composite::new(
        block,
        CompositeMethod::Over,
        (rect.x as usize, rect.y as usize),
    )
    .execute(canvas)
    .map_err(img_err("block-composite"))?;
    Ok(())
}

/// Alpha threshold above which a pixel counts as drawn content (below it is the
/// transparent backing / antialiased dust around a generated cutout).
const CONTENT_ALPHA_THRESHOLD: u8 = 16;

/// Tight bounding box `(x, y, w, h)` of the pixels with `alpha > threshold`.
/// Returns `None` for a fully-transparent image (nothing to place). This is the
/// pixel-reading unlock: a generated cutout carries large transparent margins, so
/// sizing/placing by this content box — not the nominal frame — fixes the "too
/// small / over-spaced" look.
fn content_bbox(img: &Image, threshold: u8) -> Result<Option<(u32, u32, u32, u32)>, ConduitError> {
    let (w, h) = img.dimensions();
    let a = alpha_channel(img)?;
    let (mut min_x, mut min_y) = (usize::MAX, usize::MAX);
    let (mut max_x, mut max_y) = (0usize, 0usize);
    let mut found = false;
    for y in 0..h {
        let base = y * w;
        for x in 0..w {
            if a[base + x] > threshold {
                found = true;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    if !found {
        return Ok(None);
    }
    Ok(Some((
        min_x as u32,
        min_y as u32,
        (max_x - min_x + 1) as u32,
        (max_y - min_y + 1) as u32,
    )))
}

/// Crop an RGBA image to a sub-rectangle, returning a fresh `Image`. zune-image
/// 0.5 ships codecs only, so this is a direct channel copy — the same shape as
/// the test fixtures' `Image::from_u8`.
fn crop_to(img: &Image, x: u32, y: u32, w: u32, h: u32) -> Result<Image, ConduitError> {
    let (iw, _ih) = img.dimensions();
    let channels = img.channels_ref(false);
    let chan = |i: usize, what: &str| -> Result<&[u8], ConduitError> {
        channels
            .get(i)
            .ok_or_else(|| ConduitError::ImageError(format!("crop: missing channel {what}")))?
            .reinterpret_as()
            .map_err(img_err("crop-channel"))
    };
    let r = chan(0, "R")?;
    let g = chan(1, "G")?;
    let b = chan(2, "B")?;
    let a = chan(3, "A")?;
    let (x, y, w, h) = (x as usize, y as usize, w as usize, h as usize);
    let mut px = vec![0u8; w * h * 4];
    for row in 0..h {
        for col in 0..w {
            let src = (y + row) * iw + (x + col);
            let dst = (row * w + col) * 4;
            px[dst] = r[src];
            px[dst + 1] = g[src];
            px[dst + 2] = b[src];
            px[dst + 3] = a[src];
        }
    }
    Ok(Image::from_u8(&px, w, h, ColorSpace::RGBA))
}

/// The shared front-half of strategy-driven assembly: decode the background
/// (optional Lanczos upscale), decode + content-trim + measure each piece, and
/// run the layout `strategy`. Stops short of any compositing/serialization so
/// both the raster path (`compose_with_layout`) and the SVG path
/// (`super::svg::compose_svg`) consume the identical layout. Deterministic.
pub(super) struct Arranged {
    /// Decoded + optionally-upscaled background canvas (no pieces drawn on it).
    pub canvas: Image,
    pub canvas_w: u32,
    pub canvas_h: u32,
    /// Background RGB `(mean, std)` for Reinhard color-matching the pieces.
    pub bg_stats: [(f32, f32); 3],
    /// Placements in paint order (far → near), indexing into `pieces`.
    pub placements: Vec<Placement>,
    /// Cropped, native-resolution pieces indexed by `Placement.index`.
    pub pieces: Vec<Image>,
    /// Per-piece labels aligned to `pieces` (carried through the transparent-piece
    /// filtering so an SVG layer name follows its surviving piece). `None` per
    /// piece when the caller passed no label.
    pub labels: Vec<Option<String>>,
}

pub(super) fn arrange_pieces(
    background_png: &[u8],
    piece_pngs: &[Vec<u8>],
    labels: &[Option<String>],
    strategy: &dyn LayoutStrategy,
    params: &LayoutParams,
    canvas_size: Option<(u32, u32)>,
) -> Result<Arranged, ConduitError> {
    let mut canvas = decode_rgba8(background_png, "background")?;

    // Optional output-canvas upscale (same Lanczos resize as `composite_blocks`):
    // redraw the calm base larger so crisp pieces composite at higher resolution.
    if let Some((cw, ch)) = canvas_size {
        let (bw, bh) = canvas.dimensions();
        if cw > 0 && ch > 0 && (cw, ch) != (bw as u32, bh as u32) {
            Resize::new(cw as usize, ch as usize, ResizeMethod::Lanczos3)
                .execute(&mut canvas)
                .map_err(img_err("canvas-resize"))?;
        }
    }

    let (canvas_w, canvas_h) = canvas.dimensions();
    let (canvas_w, canvas_h) = (canvas_w as u32, canvas_h as u32);
    let bg_stats = rgb_stats(&canvas)?;

    // Decode + content-trim each piece. `kept` holds the cropped images; `measured`
    // describes them for the strategy. The strategy returns indices into `kept`.
    // `kept_labels` tracks each surviving piece's label by the same filtering so an
    // SVG layer name stays paired with its piece.
    let mut kept: Vec<Image> = Vec::with_capacity(piece_pngs.len());
    let mut kept_labels: Vec<Option<String>> = Vec::with_capacity(piece_pngs.len());
    let mut measured: Vec<MeasuredPiece> = Vec::with_capacity(piece_pngs.len());
    for (i, png) in piece_pngs.iter().enumerate() {
        let img = decode_rgba8(png, &format!("piece[{i}]"))?;
        match content_bbox(&img, CONTENT_ALPHA_THRESHOLD)? {
            Some((bx, by, bw, bh)) => {
                let cropped = crop_to(&img, bx, by, bw, bh)?;
                measured.push(MeasuredPiece {
                    index: kept.len(),
                    content_w: bw,
                    content_h: bh,
                });
                kept.push(cropped);
                kept_labels.push(labels.get(i).cloned().flatten());
            }
            None => {
                log::warn!("compose: piece[{i}] is fully transparent — skipping");
            }
        }
    }

    if measured.is_empty() {
        return Err(ConduitError::ImageError(
            "compose: no non-transparent pieces to place".to_string(),
        ));
    }

    let placements = strategy.arrange((canvas_w, canvas_h), &measured, params);

    Ok(Arranged {
        canvas,
        canvas_w,
        canvas_h,
        bg_stats,
        placements,
        pieces: kept,
        labels: kept_labels,
    })
}

/// Strategy-positioned assembly: decode the background (optional upscale), then
/// for each piece decode → trim to its content bbox → measure, run the layout
/// `strategy`, and `place_block` each cropped piece at its computed cell.
/// Fully-transparent pieces are skipped. Deterministic end-to-end (no model).
pub fn compose_with_layout(
    background_png: &[u8],
    piece_pngs: &[Vec<u8>],
    strategy: &dyn LayoutStrategy,
    params: &LayoutParams,
    opts: CompositeOpts,
    canvas_size: Option<(u32, u32)>,
) -> Result<(Vec<u8>, u32, u32), ConduitError> {
    let Arranged {
        mut canvas,
        bg_stats,
        placements,
        pieces,
        ..
    } = arrange_pieces(
        background_png,
        piece_pngs,
        &[],
        strategy,
        params,
        canvas_size,
    )?;

    // Wrap in `Option` so each kept index is placed exactly once via `take()`
    // (a clone-free move out of the slot), matching the strategy's one-placement-
    // per-piece contract.
    let mut slots: Vec<Option<Image>> = pieces.into_iter().map(Some).collect();
    for pl in &placements {
        if let Some(mut img) = slots.get_mut(pl.index).and_then(Option::take) {
            place_block(
                &mut canvas,
                &mut img,
                Rect {
                    x: pl.x,
                    y: pl.y,
                    w: pl.w,
                    h: pl.h,
                },
                opts,
                &bg_stats,
            )?;
        }
    }

    // Flatten residual transparency to opaque (see `composite_blocks`): the spread
    // sits on an opaque base and is shown opaque; sub-255 alpha would checkerboard.
    canvas
        .modify_pixels_mut::<u8, _>(|_y, _x, px| {
            *px[3] = 255;
        })
        .map_err(img_err("flatten-alpha"))?;

    let (w, h) = canvas.dimensions();
    let bytes = encode_png(&canvas)?;
    Ok((bytes, w as u32, h as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a solid RGBA color as PNG for fixtures.
    fn solid_png(w: usize, h: usize, rgba: [u8; 4]) -> Vec<u8> {
        let mut px = vec![0u8; w * h * 4];
        for chunk in px.chunks_exact_mut(4) {
            chunk.copy_from_slice(&rgba);
        }
        let img = Image::from_u8(&px, w, h, ColorSpace::RGBA);
        img.write_to_vec(ImageFormat::PNG).unwrap()
    }

    /// Encode an arbitrary RGBA buffer as PNG for fixtures.
    fn rgba_png(px: &[u8], w: usize, h: usize) -> Vec<u8> {
        Image::from_u8(px, w, h, ColorSpace::RGBA)
            .write_to_vec(ImageFormat::PNG)
            .unwrap()
    }

    /// Read the RGBA pixel at (x,y) from an encoded PNG.
    fn pixel_at(png: &[u8], x: usize, y: usize) -> [u8; 4] {
        let img = decode_rgba8(png, "test").unwrap();
        let (w, _h) = img.dimensions();
        let channels = img.channels_ref(false);
        let idx = y * w + x;
        let mut out = [0u8; 4];
        for (c, ch) in channels.iter().take(4).enumerate() {
            let data: &[u8] = ch.reinterpret_as().unwrap();
            out[c] = data[idx];
        }
        out
    }

    #[test]
    fn composite_places_block_at_origin() {
        // Blue background, opaque red block at (10,10), no color-match so the
        // red survives, no feather so the origin pixel is pure block.
        let bg = solid_png(40, 30, [0, 0, 255, 255]);
        let block = solid_png(8, 8, [255, 0, 0, 255]);
        let placements = [BlockPlacement {
            png_bytes: block,
            x: 10,
            y: 10,
            w: 8,
            h: 8,
        }];
        let opts = CompositeOpts {
            color_match: false,
            feather: 0,
            shadow: false,
        };
        let (out, w, h) = composite_blocks(&bg, &placements, opts, None).unwrap();
        assert_eq!((w, h), (40, 30), "canvas dimensions preserved");
        // Pixel under the block came from the block (red), pixel outside is bg.
        assert_eq!(pixel_at(&out, 12, 12), [255, 0, 0, 255], "block pixel");
        assert_eq!(pixel_at(&out, 0, 0)[2], 255, "background pixel still blue");
    }

    #[test]
    fn composite_output_is_fully_opaque_even_with_transparent_block() {
        // Opaque blue bg, a half-transparent red block. Pre-fix the output pixel
        // under the block carried the block's alpha (~128); it must now be 255.
        let bg = solid_png(40, 30, [0, 0, 255, 255]);
        let block = solid_png(8, 8, [255, 0, 0, 128]);
        let placements = [BlockPlacement {
            png_bytes: block,
            x: 10,
            y: 10,
            w: 8,
            h: 8,
        }];
        let opts = CompositeOpts {
            color_match: false,
            feather: 0,
            shadow: false,
        };
        let (out, _, _) = composite_blocks(&bg, &placements, opts, None).unwrap();
        assert_eq!(
            pixel_at(&out, 12, 12)[3],
            255,
            "covered pixel must be opaque"
        );
        assert_eq!(pixel_at(&out, 0, 0)[3], 255, "uncovered pixel opaque too");
    }

    #[test]
    fn shadow_follows_silhouette_not_bounding_box() {
        // 80x80 white bg. A 60x60 block, transparent except an opaque black
        // square in its top-left (block-local 5..35). Placed at (10,10) with the
        // shadow on. The old bounding-box shadow greyed the whole cell; the
        // silhouette shadow must leave the far (empty) corner the background.
        let bg = solid_png(80, 80, [255, 255, 255, 255]);
        let mut px = vec![0u8; 60 * 60 * 4];
        for y in 0..60 {
            for x in 0..60 {
                if (5..35).contains(&x) && (5..35).contains(&y) {
                    let i = (y * 60 + x) * 4;
                    px[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
        let block = rgba_png(&px, 60, 60);
        let placements = [BlockPlacement {
            png_bytes: block,
            x: 10,
            y: 10,
            w: 60,
            h: 60,
        }];
        let opts = CompositeOpts {
            color_match: false,
            feather: 0,
            shadow: true,
        };
        let (out, _, _) = composite_blocks(&bg, &placements, opts, None).unwrap();

        // Far empty corner stays background — proves the shadow is not a rectangle.
        let corner = pixel_at(&out, 62, 62)[0];
        assert!(
            corner > 250,
            "empty cell corner must stay background (no grey rectangle), got {corner}"
        );
        // A transparent-but-shadowed pixel near the blob is darkened.
        let shadowed = pixel_at(&out, 45, 45)[0];
        assert!(
            (shadowed as u16) + 6 < corner as u16,
            "silhouette shadow must darken near the blob: corner={corner} shadowed={shadowed}"
        );
        // The blob interior is the opaque block.
        let blob = pixel_at(&out, 28, 28)[0];
        assert!(blob < 60, "blob interior should be opaque dark, got {blob}");
    }

    #[test]
    fn defringe_removes_white_sticker_halo() {
        // Dark bg. A cutout = red core ringed by a 2px band of half-transparent
        // WHITE (the gpt-image matte fringe). Without defringe the ring composites
        // to a light halo over the dark bg; erosion must remove it.
        let bg = solid_png(80, 80, [0, 0, 60, 255]);
        let mut px = vec![0u8; 40 * 40 * 4];
        for y in 0..40 {
            for x in 0..40 {
                let i = (y * 40 + x) * 4;
                let core = (10..30).contains(&x) && (10..30).contains(&y);
                let ring = (8..32).contains(&x) && (8..32).contains(&y) && !core;
                if core {
                    px[i..i + 4].copy_from_slice(&[220, 30, 30, 255]);
                } else if ring {
                    px[i..i + 4].copy_from_slice(&[255, 255, 255, 128]);
                }
            }
        }
        let block = rgba_png(&px, 40, 40);
        let placements = [BlockPlacement {
            png_bytes: block,
            x: 20,
            y: 20,
            w: 40,
            h: 40,
        }];
        let opts = CompositeOpts {
            color_match: false,
            feather: 0,
            shadow: false,
        };
        let (out, _, _) = composite_blocks(&bg, &placements, opts, None).unwrap();

        // Former white-ring pixel (block-local 9,20 → canvas 29,40): the ring is
        // eroded away, so the background dark blue shows — not a light halo.
        let halo = pixel_at(&out, 29, 40);
        assert!(
            halo[0] < 80,
            "white sticker halo must be removed (red channel low), got {halo:?}"
        );
        // The red core survives.
        let core = pixel_at(&out, 40, 40);
        assert!(core[0] > 140, "red core should remain, got {core:?}");
    }

    #[test]
    fn color_match_pulls_block_toward_background() {
        // A near-black block color-matched onto a bright background should get
        // lighter (its mean is pulled toward the background's).
        let bg = solid_png(20, 20, [200, 200, 200, 255]);
        let block = solid_png(6, 6, [20, 20, 20, 255]);
        let placements = [BlockPlacement {
            png_bytes: block,
            x: 4,
            y: 4,
            w: 6,
            h: 6,
        }];
        let opts = CompositeOpts {
            color_match: true,
            feather: 0,
            shadow: false,
        };
        let (out, _, _) = composite_blocks(&bg, &placements, opts, None).unwrap();
        let p = pixel_at(&out, 6, 6);
        assert!(
            p[0] > 100,
            "color-matched block should be pulled lighter, got {p:?}"
        );
    }

    #[test]
    fn content_bbox_finds_tight_box_around_blob() {
        // 40x40 transparent with an opaque blob at x:10..20, y:5..25.
        let mut px = vec![0u8; 40 * 40 * 4];
        for y in 5..25 {
            for x in 10..20 {
                let i = (y * 40 + x) * 4;
                px[i..i + 4].copy_from_slice(&[10, 200, 30, 255]);
            }
        }
        let img = decode_rgba8(&rgba_png(&px, 40, 40), "t").unwrap();
        let bbox = content_bbox(&img, CONTENT_ALPHA_THRESHOLD).unwrap();
        assert_eq!(bbox, Some((10, 5, 10, 20)));
    }

    #[test]
    fn content_bbox_none_for_fully_transparent() {
        let img = decode_rgba8(&solid_png(8, 8, [0, 0, 0, 0]), "t").unwrap();
        assert_eq!(content_bbox(&img, CONTENT_ALPHA_THRESHOLD).unwrap(), None);
    }

    #[test]
    fn crop_to_extracts_subrect() {
        let mut px = vec![0u8; 40 * 40 * 4];
        for y in 5..25 {
            for x in 10..20 {
                let i = (y * 40 + x) * 4;
                px[i..i + 4].copy_from_slice(&[10, 200, 30, 255]);
            }
        }
        let img = decode_rgba8(&rgba_png(&px, 40, 40), "t").unwrap();
        let cropped = crop_to(&img, 10, 5, 10, 20).unwrap();
        assert_eq!(cropped.dimensions(), (10, 20));
        // Top-left of the crop is the blob colour, opaque.
        let png = encode_png(&cropped).unwrap();
        assert_eq!(pixel_at(&png, 0, 0), [10, 200, 30, 255]);
    }

    #[test]
    fn compose_with_layout_places_trimmed_pieces_opaque() {
        use crate::service::image::layout::{LayoutParams, PanoramaStrategy};
        let bg = solid_png(200, 150, [0, 0, 90, 255]);
        // Two pieces, each a small opaque blob in a big transparent frame: the
        // content-trim must measure the blob, not the 60x60 frame.
        let piece = |c: [u8; 4]| {
            let mut px = vec![0u8; 60 * 60 * 4];
            for y in 20..40 {
                for x in 20..40 {
                    let i = (y * 60 + x) * 4;
                    px[i..i + 4].copy_from_slice(&c);
                }
            }
            rgba_png(&px, 60, 60)
        };
        let pieces = vec![piece([220, 40, 40, 255]), piece([40, 220, 40, 255])];
        let params = LayoutParams {
            seed: 7,
            ..Default::default()
        };
        let opts = CompositeOpts {
            color_match: false,
            feather: 0,
            shadow: false,
        };
        let (out, w, h) =
            compose_with_layout(&bg, &pieces, &PanoramaStrategy, &params, opts, None).unwrap();
        assert_eq!((w, h), (200, 150), "output matches background dims");
        assert_eq!(pixel_at(&out, 0, 0)[3], 255, "output fully opaque");
    }

    #[test]
    fn compose_with_layout_errors_when_all_pieces_transparent() {
        use crate::service::image::layout::{LayoutParams, PanoramaStrategy};
        let bg = solid_png(50, 50, [0, 0, 0, 255]);
        let blank = solid_png(20, 20, [0, 0, 0, 0]);
        let opts = CompositeOpts::default();
        let res = compose_with_layout(
            &bg,
            &[blank],
            &PanoramaStrategy,
            &LayoutParams::default(),
            opts,
            None,
        );
        assert!(
            res.is_err(),
            "all-transparent pieces should error, not panic"
        );
    }

    #[test]
    fn canvas_upscales_background_before_compositing() {
        // 40x30 blue base, requesting an 80x60 canvas: the base is Lanczos-upscaled to 80x60
        // and a red block at canvas coords (20,20) lands there; output dims are the canvas.
        let bg = solid_png(40, 30, [0, 0, 255, 255]);
        let block = solid_png(8, 8, [255, 0, 0, 255]);
        let placements = [BlockPlacement {
            png_bytes: block,
            x: 20,
            y: 20,
            w: 8,
            h: 8,
        }];
        let opts = CompositeOpts {
            color_match: false,
            feather: 0,
            shadow: false,
        };
        let (out, w, h) = composite_blocks(&bg, &placements, opts, Some((80, 60))).unwrap();
        assert_eq!((w, h), (80, 60), "output is the requested canvas size");
        assert_eq!(
            pixel_at(&out, 22, 22),
            [255, 0, 0, 255],
            "block placed at canvas coords"
        );
        assert_eq!(pixel_at(&out, 0, 0)[2], 255, "upscaled base still blue");
    }
}
