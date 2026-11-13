use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;

// Re-export public DTO from dapple-types
pub use dapple_types::model::brief_units::BriefUnitMarkdown;

/// Internal model for brief unit markdown subtype
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefUnitMarkdownModel {
    pub brief_unit_id: Uuid,
    pub markdown_text: Option<String>,
    pub fluent_message_id: Option<Uuid>,
    pub format: Option<String>,
    pub deliverable_spec: serde_json::Value,
}

impl From<BriefUnitMarkdownModel> for BriefUnitMarkdown {
    fn from(model: BriefUnitMarkdownModel) -> Self {
        Self {
            markdown_text: model.markdown_text,
            fluent_message_id: model.fluent_message_id,
            format: model.format,
            deliverable_spec: model.deliverable_spec,
        }
    }
}

/// Insert arguments for brief unit markdown subtype
#[derive(Clone, Debug)]
pub struct InsertBriefUnitMarkdownArgs {
    pub markdown_text: Option<String>,
    pub fluent_message_id: Option<Uuid>,
    pub format: Option<String>,
    pub deliverable_spec: serde_json::Value,
}
