//! Schema types for the guide render endpoint.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Request body for rendering a guide to PDF.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RenderGuideSchema {
    /// The render profile key to use (e.g., "consumer", "agent", "internal").
    pub profile: String,
}

/// Response for a successful synchronous guide render.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RenderGuideResponse {
    pub status: String,
    /// The content type of the rendered output.
    pub content_type: String,
    /// The number of bytes in the rendered PDF.
    pub size_bytes: usize,
}
