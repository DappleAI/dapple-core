//! Error types for the render crate.

use thiserror::Error;

/// Errors that can occur during PDF rendering.
#[derive(Error, Debug)]
pub enum RenderError {
    /// Font file(s) could not be found or loaded.
    #[error("Font not found: {0}")]
    FontNotFound(String),

    /// An image could not be decoded or embedded.
    #[error("Image processing error: {0}")]
    ImageProcessing(String),

    /// PDF generation failed.
    #[error("PDF generation error: {0}")]
    PdfGeneration(String),

    /// The generated Typst source failed to compile. Every line of that
    /// source is machine-generated, so this indicates a transform bug — the
    /// message carries line:column plus the offending generated line so a
    /// single conduit log line is debuggable.
    #[error("Typst compile error: {0}")]
    TypstCompile(String),

    /// Style configuration is invalid.
    #[error("Invalid style configuration: {0}")]
    InvalidStyle(String),
}
