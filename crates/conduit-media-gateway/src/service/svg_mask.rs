//! Luma-matte rasterization for composition layers.
//!
//! [`rasterize_mask`] renders any SVG asset once at the requested resolution
//! into a white-shapes-on-black PNG (any painted pixel → white by coverage),
//! the luma matte ffmpeg's `alphamerge` consumes (inverted in-graph when the
//! layer asks for it). Matte motion is applied at composite time as crop
//! expressions of `t` over a slightly oversized raster — never per-frame
//! re-rasterization.
//!
//! This module is a generic mechanism: it knows nothing about what the SVG
//! depicts. Geometry *extraction* (e.g. reading dot windows out of a logo
//! mask) is policy and lives with the strategy compilers in conduit-oximedia.

use crate::error::MediaGatewayError;

fn parse_err(msg: impl std::fmt::Display) -> MediaGatewayError {
    MediaGatewayError::InvalidRequest(format!("matte svg: {msg}"))
}

/// Rasterize an SVG at `width`x`height` into a white-on-black luma matte:
/// every painted pixel (any color, any partial alpha) becomes white scaled by
/// its coverage; unpainted stays black. Returned as encoded PNG bytes for
/// ffmpeg to consume as an input.
pub fn rasterize_mask(svg: &[u8], width: u32, height: u32) -> Result<Vec<u8>, MediaGatewayError> {
    let pixmap = rasterize_matte_pixmap(svg, width, height)?;
    pixmap
        .encode_png()
        .map_err(|e| MediaGatewayError::MediaTool(format!("matte png encode: {e}")))
}

/// The pixmap half of [`rasterize_mask`], separated so tests can assert actual
/// pixel values without a PNG decoder.
fn rasterize_matte_pixmap(
    svg: &[u8],
    width: u32,
    height: u32,
) -> Result<resvg::tiny_skia::Pixmap, MediaGatewayError> {
    if width == 0 || height == 0 {
        return Err(parse_err("matte raster size must be non-zero"));
    }
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default())
        .map_err(|e| parse_err(format!("unparseable svg: {e}")))?;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| parse_err("matte raster size out of range"))?;
    let size = tree.size();
    let sx = f64::from(width) / f64::from(size.width());
    let sy = f64::from(height) / f64::from(size.height());
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(sx as f32, sy as f32),
        &mut pixmap.as_mut(),
    );

    // Coverage → luma: painted (any color) becomes white by its alpha.
    for px in pixmap.pixels_mut() {
        let a = px.alpha();
        *px = resvg::tiny_skia::PremultipliedColorU8::from_rgba(a, a, a, 255)
            .expect("a <= 255 is always premultiplied-valid");
    }
    Ok(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100">
      <rect width="200" height="100" fill="none"/>
      <circle id="alpha" cx="50" cy="50" r="20" fill="#123456"/>
      <circle cx="150" cy="50" r="10" fill="black"/>
      <ellipse id="wide" cx="100" cy="25" rx="30" ry="10" fill="red"/>
    </svg>"##;

    #[test]
    fn rejects_garbage() {
        assert!(rasterize_mask(b"not svg", 64, 64).is_err());
    }

    #[test]
    fn matte_is_white_inside_shapes_and_black_outside() {
        let pixmap = rasterize_matte_pixmap(MASK.as_bytes(), 200, 100).unwrap();
        let px = |x: u32, y: u32| pixmap.pixel(x, y).unwrap();

        // Centre of the first circle → white.
        let inside = px(50, 50);
        assert!(inside.red() > 200 && inside.green() > 200 && inside.blue() > 200);
        // Far corner (unpainted) → black.
        let outside = px(5, 90);
        assert!(outside.red() < 10 && outside.green() < 10 && outside.blue() < 10);
        // Inside the ellipse → white even though it was red.
        let ell = px(100, 25);
        assert!(ell.red() > 200 && ell.blue() > 200);
    }

    #[test]
    fn rasterize_encodes_png() {
        let png = rasterize_mask(MASK.as_bytes(), 128, 64).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }
}
