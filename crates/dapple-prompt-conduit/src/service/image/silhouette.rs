//! Silhouette clipping for the reveal collage (`/compose` silhouette option).
//!
//! The video-quilt's payoff: pattern snapshots are composed panorama-style,
//! then the whole spread is clipped to an operator-uploaded silhouette SVG
//! (the rocket) — "the SVG overlay trick". Deterministic; no model touches the
//! pixels. The silhouette is scaled to FIT the canvas (aspect preserved,
//! centered), so the payoff mark is exact and never cropped.
//!
//! Both serializations use the same rasterized coverage matte:
//! - raster: the composed PNG's alpha is REPLACED by the matte (outside the
//!   silhouette → transparent);
//! - SVG document: the matte rides as an inline luminance `<mask>` wrapping
//!   the layout's layers, so the operator can still move pieces underneath —
//!   or delete the mask attribute to see the whole spread.

use super::composite::encode_png;
use crate::error::ConduitError;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use zune_core::bit_depth::BitDepth;
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_image::image::Image;

fn sil_err(msg: impl std::fmt::Display) -> ConduitError {
    ConduitError::ImageError(format!("silhouette: {msg}"))
}

/// Rasterize the silhouette SVG to a coverage matte at `w`x`h`: one alpha byte
/// per pixel (row-major), 255 inside the painted silhouette, 0 outside. The
/// silhouette is contain-fit and centered.
pub fn rasterize_silhouette_matte(svg: &[u8], w: u32, h: u32) -> Result<Vec<u8>, ConduitError> {
    if w == 0 || h == 0 {
        return Err(sil_err("matte size must be non-zero"));
    }
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default())
        .map_err(|e| sil_err(format!("unparseable svg: {e}")))?;

    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(w, h).ok_or_else(|| sil_err("matte size out of range"))?;
    let size = tree.size();
    // Contain-fit + center: the mark is exact, never cropped.
    let scale =
        (f64::from(w) / f64::from(size.width())).min(f64::from(h) / f64::from(size.height()));
    let tx = (f64::from(w) - f64::from(size.width()) * scale) / 2.0;
    let ty = (f64::from(h) - f64::from(size.height()) * scale) / 2.0;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale as f32, scale as f32)
            .post_translate(tx as f32, ty as f32),
        &mut pixmap.as_mut(),
    );

    Ok(pixmap.pixels().iter().map(|p| p.alpha()).collect())
}

/// Clip a composed PNG to the silhouette: alpha := matte. Outside the mark
/// becomes fully transparent; inside keeps the collage.
pub fn apply_silhouette_png(
    composed_png: &[u8],
    silhouette_svg: &[u8],
) -> Result<Vec<u8>, ConduitError> {
    let mut img = Image::read(ZCursor::new(composed_png), DecoderOptions::default())
        .map_err(|e| sil_err(format!("decode composed png: {e:?}")))?;
    img.convert_depth(BitDepth::Eight)
        .map_err(|e| sil_err(format!("to-depth8: {e:?}")))?;
    img.convert_color(ColorSpace::RGBA)
        .map_err(|e| sil_err(format!("to-rgba: {e:?}")))?;

    let (w, h) = img.dimensions();
    let matte = rasterize_silhouette_matte(silhouette_svg, w as u32, h as u32)?;

    img.modify_pixels_mut::<u8, _>(|y, x, px| {
        *px[3] = matte[y * w + x];
    })
    .map_err(|e| sil_err(format!("apply matte: {e:?}")))?;

    encode_png(&img)
}

/// Wrap a composed SVG layout document's content in a luminance `<mask>` built
/// from the silhouette matte (inline data-URI PNG). Pure text surgery on the
/// emitter's output: defs + an opening `<g mask=…>` right after the root
/// `<svg …>` tag, `</g>` before the final `</svg>`. The pieces stay movable
/// layers; deleting the group's `mask` attribute un-clips the spread.
pub fn apply_silhouette_svg(
    svg_doc: &[u8],
    silhouette_svg: &[u8],
    w: u32,
    h: u32,
) -> Result<Vec<u8>, ConduitError> {
    let doc = std::str::from_utf8(svg_doc).map_err(|e| sil_err(format!("svg not utf-8: {e}")))?;

    let open_start = doc
        .find("<svg")
        .ok_or_else(|| sil_err("document has no <svg> root"))?;
    let open_end = doc[open_start..]
        .find('>')
        .map(|i| open_start + i + 1)
        .ok_or_else(|| sil_err("unterminated <svg> tag"))?;
    let close = doc
        .rfind("</svg>")
        .ok_or_else(|| sil_err("document has no </svg>"))?;
    if close < open_end {
        return Err(sil_err("malformed svg document"));
    }

    // Luminance mask: white = keep. Encode the matte as a gray PNG data URI.
    let matte = rasterize_silhouette_matte(silhouette_svg, w, h)?;
    let mut rgba = Vec::with_capacity(matte.len() * 4);
    for a in &matte {
        rgba.extend_from_slice(&[*a, *a, *a, 255]);
    }
    let matte_img = Image::from_u8(&rgba, w as usize, h as usize, ColorSpace::RGBA);
    let matte_png = encode_png(&matte_img)?;
    let matte_uri = format!("data:image/png;base64,{}", STANDARD.encode(matte_png));

    let injection = format!(
        r#"<defs><mask id="dapple-silhouette" maskUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{h}"><image x="0" y="0" width="{w}" height="{h}" href="{matte_uri}"/></mask></defs><g id="silhouette-clipped" mask="url(#dapple-silhouette)">"#
    );

    let mut out = String::with_capacity(doc.len() + injection.len() + 8);
    out.push_str(&doc[..open_end]);
    out.push_str(&injection);
    out.push_str(&doc[open_end..close]);
    out.push_str("</g>");
    out.push_str(&doc[close..]);
    Ok(out.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A right-half-filled square silhouette: everything left of x=10 is empty.
    const SILHOUETTE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20">
      <rect x="10" y="0" width="10" height="20" fill="black"/>
    </svg>"#;

    fn solid_png(w: usize, h: usize, rgba: [u8; 4]) -> Vec<u8> {
        let mut px = vec![0u8; w * h * 4];
        for chunk in px.chunks_exact_mut(4) {
            chunk.copy_from_slice(&rgba);
        }
        Image::from_u8(&px, w, h, ColorSpace::RGBA)
            .write_to_vec(zune_image::codecs::ImageFormat::PNG)
            .unwrap()
    }

    fn alpha_at(png: &[u8], x: usize, y: usize) -> u8 {
        let mut img = Image::read(ZCursor::new(png), DecoderOptions::default()).unwrap();
        img.convert_depth(BitDepth::Eight).unwrap();
        img.convert_color(ColorSpace::RGBA).unwrap();
        let (w, _) = img.dimensions();
        let channels = img.channels_ref(false);
        let a: &[u8] = channels[3].reinterpret_as().unwrap();
        a[y * w + x]
    }

    #[test]
    fn matte_covers_painted_region_only() {
        // Square silhouette on a square canvas: no letterbox; right half filled.
        let matte = rasterize_silhouette_matte(SILHOUETTE.as_bytes(), 40, 40).unwrap();
        assert_eq!(matte.len(), 40 * 40);
        assert_eq!(matte[20 * 40 + 30], 255, "inside (right half)");
        assert_eq!(matte[20 * 40 + 5], 0, "outside (left half)");
    }

    #[test]
    fn matte_contain_fits_and_centers() {
        // Square silhouette on a wide canvas → letterboxed left/right; the
        // silhouette occupies the centered 40-wide band (x 20..60).
        let matte = rasterize_silhouette_matte(SILHOUETTE.as_bytes(), 80, 40).unwrap();
        assert_eq!(matte[20 * 80 + 10], 0, "left letterbox is empty");
        assert_eq!(matte[20 * 80 + 50], 255, "right half of the centered band");
        assert_eq!(matte[20 * 80 + 25], 0, "left half of the centered band");
        assert_eq!(matte[20 * 80 + 70], 0, "right letterbox is empty");
    }

    #[test]
    fn png_clip_makes_outside_transparent() {
        let composed = solid_png(40, 40, [200, 100, 50, 255]);
        let clipped = apply_silhouette_png(&composed, SILHOUETTE.as_bytes()).unwrap();
        assert_eq!(alpha_at(&clipped, 30, 20), 255, "inside stays opaque");
        assert_eq!(alpha_at(&clipped, 5, 20), 0, "outside goes transparent");
    }

    #[test]
    fn svg_injection_wraps_content_in_a_mask() {
        let doc = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect width="40" height="40" fill="red"/></svg>"#;
        let out = apply_silhouette_svg(doc.as_bytes(), SILHOUETTE.as_bytes(), 40, 40).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains(r#"<mask id="dapple-silhouette""#), "{text}");
        assert!(text.contains(r#"<g id="silhouette-clipped" mask="url(#dapple-silhouette)">"#));
        assert!(text.contains("data:image/png;base64,"));
        assert!(text.trim_end().ends_with("</svg>"));
        // The wrapped content is still present.
        assert!(text.contains(r#"<rect width="40" height="40" fill="red"/>"#));
        // The group closes before the root closes.
        let g_close = text.rfind("</g>").unwrap();
        let svg_close = text.rfind("</svg>").unwrap();
        assert!(g_close < svg_close);
    }

    #[test]
    fn deterministic() {
        let composed = solid_png(40, 40, [1, 2, 3, 255]);
        assert_eq!(
            apply_silhouette_png(&composed, SILHOUETTE.as_bytes()).unwrap(),
            apply_silhouette_png(&composed, SILHOUETTE.as_bytes()).unwrap()
        );
    }

    #[test]
    fn rejects_garbage_silhouette() {
        let composed = solid_png(8, 8, [0, 0, 0, 255]);
        assert!(apply_silhouette_png(&composed, b"not svg").is_err());
        assert!(apply_silhouette_svg(b"<svg></svg>", b"not svg", 8, 8).is_err());
    }
}
