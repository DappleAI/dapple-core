//! SVG layout-container emitter for strategy-driven assembly.
//!
//! Where [`super::composite::compose_with_layout`] FLATTENS the laid-out pieces
//! into one raster PNG, this emits the *same* deterministic layout as an SVG in
//! which every piece stays an independent, named, movable object — so an operator
//! can pull the file down and finish the composition by hand (nudge, rescale,
//! restack, delete) in a vector editor. That hand-finishing is the point: the
//! generated placement is a starting draft, not the deliverable. No model touches
//! the pixels (the inviolability principle); this is strictly a different
//! serialization of the layout the panorama strategy already computed.
//!
//! The file is fully SELF-CONTAINED: the backdrop and every piece are embedded as
//! inline `data:image/png;base64,…` URIs (the actual bytes), never `xlink:href`
//! URLs to a dapple asset endpoint. A downloaded `.svg` opens offline,
//! off-platform, with no auth. Pieces are embedded at native (content-trimmed)
//! resolution so they scale up cleanly in the editor; the `<image width/height>`
//! sizes each onto the canvas. `opts.feather` is a no-op here (cutouts already
//! carry soft alpha); `opts.shadow` becomes an editable drop-shadow filter
//! (spelled out with widely-supported primitives, not the `feDropShadow`
//! shorthand) rather than baked pixels, so the operator can tweak or remove the
//! grounding.

use super::composite::{arrange_pieces, clean_piece, encode_png, Arranged};
use super::layout::{LayoutParams, LayoutStrategy};
use super::CompositeOpts;
use crate::error::ConduitError;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use zune_image::image::Image;

/// Id of the shared drop-shadow filter referenced by each piece group when
/// `opts.shadow`. The filter is the classic feGaussianBlur/feOffset/feFlood/
/// feComposite/feMerge expansion (portable to Inkscape 0.92 and old librsvg),
/// whose `dx/dy/σ/opacity` mirror the raster shadow constants so the SVG grounds
/// pieces like the PNG path did.
const SHADOW_FILTER_ID: &str = "pp-contact-shadow";

/// Encode an RGBA image as a self-contained `data:image/png;base64,…` URI — the
/// actual bytes inline, NOT a reference to a dapple asset endpoint.
fn data_uri_png(img: &Image) -> Result<String, ConduitError> {
    let png = encode_png(img)?;
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(png)))
}

/// XML-escape text for use in an attribute value or element body.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Build a safe, unique element `id` from a 1-based piece index and its label
/// (ascii-alnum kept + lowercased, everything else → '-'). Never empty.
fn slug_id(index: usize, label: Option<&str>) -> String {
    let base: String = label
        .unwrap_or("")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = base.trim_matches('-');
    let n = index + 1;
    if trimmed.is_empty() {
        format!("piece-{n}")
    } else {
        format!("piece-{n}-{trimmed}")
    }
}

/// Emit the deterministic panorama layout as a self-contained SVG. Returns the
/// SVG bytes plus the canvas `(width, height)`. Mirrors `compose_with_layout`'s
/// inputs and reuses its `arrange_pieces` front half, so the placement is
/// byte-identical to the raster path — only the serialization differs.
pub fn compose_svg(
    background_png: &[u8],
    piece_pngs: &[Vec<u8>],
    labels: &[Option<String>],
    strategy: &dyn LayoutStrategy,
    params: &LayoutParams,
    opts: CompositeOpts,
    canvas_size: Option<(u32, u32)>,
) -> Result<(Vec<u8>, u32, u32), ConduitError> {
    let Arranged {
        canvas,
        canvas_w,
        canvas_h,
        bg_stats,
        placements,
        pieces,
        labels: kept_labels,
    } = arrange_pieces(
        background_png,
        piece_pngs,
        labels,
        strategy,
        params,
        canvas_size,
    )?;

    let (cw, ch) = (canvas_w, canvas_h);
    let backdrop_uri = data_uri_png(&canvas)?;

    let mut svg = String::new();
    svg.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" \
         xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" \
         width=\"{cw}\" height=\"{ch}\" viewBox=\"0 0 {cw} {ch}\">\n"
    ));

    // Editable contact-shadow filter (only when requested). We spell out the
    // classic multi-primitive drop shadow (feGaussianBlur → feOffset → feFlood →
    // feComposite → feMerge) rather than the `feDropShadow` SHORTHAND: the visual
    // is identical (offset dx=6/dy=6, blur σ=10, black @ 0.43), but `feDropShadow`
    // is only supported in Inkscape >= 1.0 and librsvg >= 2.46 — older renderers
    // render the whole filtered element BLANK (the object stays in the layer tree
    // but its pixels vanish), which made every scene invisible in Inkscape while
    // Chrome looked perfect. The expanded form is understood everywhere. The final
    // feMergeNode order paints the shadow BEHIND the source graphic.
    if opts.shadow {
        svg.push_str(&format!(
            "  <defs><filter id=\"{SHADOW_FILTER_ID}\" x=\"-20%\" y=\"-20%\" \
             width=\"140%\" height=\"140%\">\
             <feGaussianBlur in=\"SourceAlpha\" stdDeviation=\"10\" result=\"pp-blur\"/>\
             <feOffset in=\"pp-blur\" dx=\"6\" dy=\"6\" result=\"pp-off\"/>\
             <feFlood flood-color=\"black\" flood-opacity=\"0.43\" result=\"pp-color\"/>\
             <feComposite in=\"pp-color\" in2=\"pp-off\" operator=\"in\" result=\"pp-shadow\"/>\
             <feMerge><feMergeNode in=\"pp-shadow\"/><feMergeNode in=\"SourceGraphic\"/></feMerge>\
             </filter></defs>\n"
        ));
    }

    // Backdrop layer (full canvas), embedded inline.
    svg.push_str(&format!(
        "  <image id=\"backdrop\" x=\"0\" y=\"0\" width=\"{cw}\" height=\"{ch}\" \
         xlink:href=\"{backdrop_uri}\"/>\n"
    ));

    // One named, movable group per piece, in paint order (document order = z-order).
    let mut slots: Vec<Option<Image>> = pieces.into_iter().map(Some).collect();
    let filter_attr = if opts.shadow {
        format!(" filter=\"url(#{SHADOW_FILTER_ID})\"")
    } else {
        String::new()
    };
    for pl in &placements {
        // Each kept index is placed exactly once; take it (clone-free move out).
        let Some(mut img) = slots.get_mut(pl.index).and_then(Option::take) else {
            continue;
        };
        // Clean the cutout edges + optional grade at NATIVE resolution (no resize):
        // the editor scales the embedded full-res raster via the <image> box, so a
        // piece blown up in Illustrator/Inkscape stays crisp.
        clean_piece(&mut img, opts.color_match, &bg_stats)?;
        let uri = data_uri_png(&img)?;

        let label = kept_labels
            .get(pl.index)
            .and_then(|l| l.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let id = slug_id(pl.index, label);
        let label_attr = match label {
            Some(h) => format!(" inkscape:label=\"{}\"", xml_escape(h)),
            None => String::new(),
        };
        let title = match label {
            Some(h) => format!("<title>{}</title>", xml_escape(h)),
            None => String::new(),
        };
        let (x, y, w, h) = (pl.x, pl.y, pl.w, pl.h);
        svg.push_str(&format!(
            "  <g id=\"{id}\"{label_attr}{filter_attr}>{title}\
             <image x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" \
             preserveAspectRatio=\"none\" xlink:href=\"{uri}\"/></g>\n"
        ));
    }

    svg.push_str("</svg>\n");
    Ok((svg.into_bytes(), cw, ch))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::image::layout::PanoramaStrategy;
    use zune_core::colorspace::ColorSpace;
    use zune_image::codecs::ImageFormat;

    fn solid_png(w: usize, h: usize, rgba: [u8; 4]) -> Vec<u8> {
        let mut px = vec![0u8; w * h * 4];
        for chunk in px.chunks_exact_mut(4) {
            chunk.copy_from_slice(&rgba);
        }
        Image::from_u8(&px, w, h, ColorSpace::RGBA)
            .write_to_vec(ImageFormat::PNG)
            .unwrap()
    }

    /// An opaque blob centred in a transparent frame (so content-trim keeps it).
    fn blob_png(w: usize, h: usize, rgba: [u8; 4]) -> Vec<u8> {
        let mut px = vec![0u8; w * h * 4];
        for y in (h / 4)..(3 * h / 4) {
            for x in (w / 4)..(3 * w / 4) {
                let i = (y * w + x) * 4;
                px[i..i + 4].copy_from_slice(&rgba);
            }
        }
        Image::from_u8(&px, w, h, ColorSpace::RGBA)
            .write_to_vec(ImageFormat::PNG)
            .unwrap()
    }

    fn as_string(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn compose_svg_embeds_self_contained_named_layers() {
        let bg = solid_png(200, 150, [10, 20, 90, 255]);
        let pieces = vec![
            blob_png(60, 60, [220, 40, 40, 255]),
            blob_png(60, 60, [40, 220, 40, 255]),
        ];
        let labels = vec![Some("Scene 1".to_string()), Some("Scene 2".to_string())];
        let opts = CompositeOpts {
            color_match: false,
            feather: 0,
            shadow: true,
        };
        let params = LayoutParams {
            seed: 7,
            ..Default::default()
        };
        let (bytes, w, h) = compose_svg(
            &bg,
            &pieces,
            &labels,
            &PanoramaStrategy,
            &params,
            opts,
            None,
        )
        .unwrap();
        assert_eq!((w, h), (200, 150));

        let s = as_string(&bytes);
        assert!(s.starts_with("<?xml"));
        assert!(s.contains("viewBox=\"0 0 200 150\""));
        // Backdrop + 2 pieces, all embedded inline (self-contained).
        assert_eq!(s.matches("data:image/png;base64,").count(), 3);
        // NOTHING references a dapple asset endpoint or any non-data href.
        assert!(!s.contains("/api/assets/"));
        assert!(!s.contains("xlink:href=\"http"));
        assert!(!s.contains("xlink:href=\"/"));
        // Two named, movable layers.
        assert_eq!(s.matches("<g ").count(), 2);
        assert!(s.contains("inkscape:label=\"Scene 1\""));
        assert!(s.contains("<title>Scene 2</title>"));
        // Editable contact-shadow filter, defined once and referenced.
        assert!(s.contains("id=\"pp-contact-shadow\""));
        assert!(s.contains("filter=\"url(#pp-contact-shadow)\""));
        // Portable expansion, NOT the poorly-supported `feDropShadow` shorthand
        // (the latter renders blank in Inkscape < 1.0 / librsvg < 2.46).
        assert!(s.contains("feGaussianBlur"));
        assert!(!s.contains("feDropShadow"));
    }

    #[test]
    fn compose_svg_escapes_labels_and_omits_filter_without_shadow() {
        let bg = solid_png(120, 100, [0, 0, 0, 255]);
        let pieces = vec![blob_png(40, 40, [200, 200, 200, 255])];
        let labels = vec![Some("Scene 3 & \"props\" <x>".to_string())];
        let opts = CompositeOpts {
            color_match: false,
            feather: 0,
            shadow: false,
        };
        let (bytes, _, _) = compose_svg(
            &bg,
            &pieces,
            &labels,
            &PanoramaStrategy,
            &LayoutParams::default(),
            opts,
            None,
        )
        .unwrap();
        let s = as_string(&bytes);
        assert!(s.contains("Scene 3 &amp; &quot;props&quot; &lt;x&gt;"));
        // No shadow → no filter def and no per-group filter reference.
        assert!(!s.contains("pp-contact-shadow"));
        assert!(!s.contains("<feDropShadow"));
    }

    #[test]
    fn compose_svg_unlabeled_pieces_still_emit_movable_groups() {
        let bg = solid_png(160, 120, [30, 30, 30, 255]);
        let pieces = vec![blob_png(50, 50, [100, 150, 200, 255])];
        // No labels supplied: group still emitted, generically id'd, no <title>.
        let opts = CompositeOpts {
            color_match: false,
            feather: 0,
            shadow: false,
        };
        let (bytes, _, _) = compose_svg(
            &bg,
            &pieces,
            &[],
            &PanoramaStrategy,
            &LayoutParams::default(),
            opts,
            None,
        )
        .unwrap();
        let s = as_string(&bytes);
        assert_eq!(s.matches("<g ").count(), 1);
        assert!(s.contains("id=\"piece-1\""));
        assert!(!s.contains("<title>"));
        assert!(!s.contains("inkscape:label="));
    }
}
