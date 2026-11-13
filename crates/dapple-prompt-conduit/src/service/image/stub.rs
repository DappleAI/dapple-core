//! Deterministic stub image backend for tests and key-less environments.
//!
//! `generate` produces a solid-color PNG at the requested size, with the color
//! derived from the prompt so distinct prompts yield distinct bytes. Built on
//! zune (the same stack the compositing module uses) so the conduit's image work
//! is single-stack. Generate-only: there is no `edit` — a model never reprocesses
//! existing pixels (see `service/image/mod.rs`).

use super::{ImageBackend, ImageGenRequest};
use crate::error::ConduitError;
use async_trait::async_trait;
use zune_core::colorspace::ColorSpace;
use zune_image::codecs::ImageFormat;
use zune_image::image::Image;

/// Parse a `WIDTHxHEIGHT` size string, clamping to a small range so stub
/// renders stay fast even if a caller asks for a large canvas.
fn parse_size(size: &str) -> (usize, usize) {
    let mut parts = size.split(['x', 'X']);
    let w = parts.next().and_then(|s| s.trim().parse::<usize>().ok());
    let h = parts.next().and_then(|s| s.trim().parse::<usize>().ok());
    let clamp = |v: usize| v.clamp(1, 64);
    match (w, h) {
        (Some(w), Some(h)) => (clamp(w), clamp(h)),
        _ => (16, 16),
    }
}

pub struct StubImageBackend;

impl StubImageBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StubImageBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ImageBackend for StubImageBackend {
    async fn generate(&self, request: &ImageGenRequest) -> Result<Vec<u8>, ConduitError> {
        let (w, h) = parse_size(&request.size);
        // Derive a stable color from the prompt so different prompts differ.
        let seed = request
            .prompt
            .bytes()
            .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
        let color = [
            (seed & 0xff) as u8,
            ((seed >> 8) & 0xff) as u8,
            ((seed >> 16) & 0xff) as u8,
            255u8,
        ];
        let mut px = vec![0u8; w * h * 4];
        for chunk in px.chunks_exact_mut(4) {
            chunk.copy_from_slice(&color);
        }
        let img = Image::from_u8(&px, w, h, ColorSpace::RGBA);
        img.write_to_vec(ImageFormat::PNG)
            .map_err(|e| ConduitError::ImageError(format!("stub PNG encode failed: {e:?}")))
    }

    /// The stub is the service's unconditional fallback, so it claims anything.
    fn supports(&self, _request: &ImageGenRequest) -> bool {
        true
    }

    fn name(&self) -> &str {
        "stub-image"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zune_core::bytestream::ZCursor;
    use zune_core::options::DecoderOptions;

    fn decoded_dimensions(bytes: &[u8]) -> (usize, usize) {
        Image::read(ZCursor::new(bytes), DecoderOptions::default())
            .unwrap()
            .dimensions()
    }

    #[tokio::test]
    async fn stub_generate_returns_png_of_requested_size() {
        let backend = StubImageBackend::new();
        let req = ImageGenRequest {
            provider: None,
            prompt: "a teeming market square".to_string(),
            model: "stub".to_string(),
            size: "32x24".to_string(),
            background: None,
            quality: None,
            seed: None,
            negative_prompt: None,
            provider_config: None,
        };
        let bytes = backend.generate(&req).await.unwrap();
        assert_eq!(decoded_dimensions(&bytes), (32, 24));
    }

    #[tokio::test]
    async fn stub_generate_is_prompt_sensitive() {
        let backend = StubImageBackend::new();
        let mk = |p: &str| ImageGenRequest {
            provider: None,
            prompt: p.to_string(),
            model: "stub".to_string(),
            size: "16x16".to_string(),
            background: None,
            quality: None,
            seed: None,
            negative_prompt: None,
            provider_config: None,
        };
        let a = backend.generate(&mk("alpha")).await.unwrap();
        let b = backend.generate(&mk("beta")).await.unwrap();
        assert_ne!(a, b, "distinct prompts should yield distinct stub images");
    }
}
