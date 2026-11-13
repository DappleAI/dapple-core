use crate::{DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for brief unit input choice
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefUnitInputChoice {
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_fluent_message_id: Option<Uuid>,
    pub value_text: String,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

/// Public DTO for brief unit markdown subtype
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefUnitMarkdown {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fluent_message_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default)]
    pub deliverable_spec: serde_json::Value,
}

/// Public DTO for brief unit notice subtype
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefUnitNotice {
    /// Notice level: "info", "warning", "error", "success"
    pub level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_fluent_message_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail_fluent_message_id: Option<Uuid>,
}

/// Public DTO for brief unit input subtype
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefUnitInput {
    /// Control type: "text", "textarea", "select", "radio", "checkbox", "number", "date", "file"
    pub control_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_fluent_message_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help_fluent_message_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder_fluent_message_id: Option<Uuid>,
    #[serde(default)]
    pub control_spec: serde_json::Value,
    #[serde(default)]
    pub choices: Vec<BriefUnitInputChoice>,
}

/// Public DTO for media source
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaSource {
    Asset {
        asset_id: Uuid,
    },
    Url {
        url: String,
    },
    /// Dynamic media source resolved from a value slot binding.
    /// The client looks up the DispatchIntentBinding for this value_slot_id
    /// and extracts the asset_id from binding.asset_items.
    ValueSlot {
        value_slot_id: Uuid,
    },
}

/// Public DTO for media item poster
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefUnitMediaItemPoster {
    pub id: Uuid,
    pub source: MediaSource,
}

/// Public DTO for media item
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefUnitMediaItem {
    pub id: Uuid,
    /// Media type: "image", "video", "audio", "document", "file"
    pub media_kind: String,
    pub source: MediaSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster: Option<BriefUnitMediaItemPoster>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_text_fluent_message_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption_fluent_message_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_fluent_message_id: Option<Uuid>,
}

/// Public DTO for brief unit media subtype
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefUnitMedia {
    pub items: Vec<BriefUnitMediaItem>,
}

/// Public enum for brief unit subtype
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BriefUnitSubtype {
    Markdown(BriefUnitMarkdown),
    Notice(BriefUnitNotice),
    Input(BriefUnitInput),
    Media(BriefUnitMedia),
    None,
}

/// Public DTO for brief unit
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefUnit {
    pub id: Uuid,
    pub unit_key: String,
    #[serde(default)]
    pub layout: serde_json::Value,
    #[serde(default)]
    pub requires: serde_json::Value,
    #[serde(default)]
    pub extensions: serde_json::Value,
    pub subtype: BriefUnitSubtype,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl DataTransferObject for BriefUnit {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
