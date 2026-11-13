//! Models for render profiles.
//!
//! Render profiles define which blocks to include and how to style
//! the rendered PDF output for a guide.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Internal model for a render profile row.
#[derive(Debug, Clone)]
pub struct RenderProfileModel {
    pub id: i16,
    pub key: String,
    pub description: Option<String>,
    pub tag_filter: serde_json::Value,
    pub style_config: serde_json::Value,
    pub created_at: OffsetDateTime,
}

/// Public DTO for a render profile.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderProfile {
    pub key: String,
    pub description: Option<String>,
    pub tag_filter: Vec<String>,
    pub style_config: serde_json::Value,
}

impl From<RenderProfileModel> for RenderProfile {
    fn from(m: RenderProfileModel) -> Self {
        let tag_filter = m
            .tag_filter
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Self {
            key: m.key,
            description: m.description,
            tag_filter,
            style_config: m.style_config,
        }
    }
}
