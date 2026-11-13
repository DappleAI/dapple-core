use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Create Schemas (POST body)
// ============================================================================

/// Markdown subtype configuration
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBriefUnitMarkdownSchema {
    /// Static markdown text (mutually exclusive with fluent_message_id)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown_text: Option<String>,
    /// Fluent message for localized content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fluent_message_id: Option<Uuid>,
    /// Format: "commonmark" or "gfm"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Additional deliverable specification
    #[serde(default)]
    pub deliverable_spec: serde_json::Value,
}

/// Notice subtype configuration
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBriefUnitNoticeSchema {
    /// Notice level: "info", "warning", "error", "success"
    pub level: String,
    /// Static title text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_text: Option<String>,
    /// Fluent message for localized title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_fluent_message_id: Option<Uuid>,
    /// Static detail text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail_text: Option<String>,
    /// Fluent message for localized detail
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail_fluent_message_id: Option<Uuid>,
}

/// Input choice configuration (for select, radio, checkbox)
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBriefUnitInputChoiceSchema {
    /// Static label text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_text: Option<String>,
    /// Fluent message for localized label
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_fluent_message_id: Option<Uuid>,
    /// The value to submit when selected
    pub value_text: String,
}

/// Input subtype configuration
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBriefUnitInputSchema {
    /// Control type: "text", "textarea", "select", "radio", "checkbox", "number", "date", "file"
    pub control_type: String,
    /// Static label text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_text: Option<String>,
    /// Fluent message for localized label
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_fluent_message_id: Option<Uuid>,
    /// Static help text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help_text: Option<String>,
    /// Fluent message for localized help
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help_fluent_message_id: Option<Uuid>,
    /// Static placeholder text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder_text: Option<String>,
    /// Fluent message for localized placeholder
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder_fluent_message_id: Option<Uuid>,
    /// Additional control configuration
    #[serde(default)]
    pub control_spec: serde_json::Value,
    /// Choices for select/radio/checkbox controls
    #[serde(default)]
    pub choices: Vec<CreateBriefUnitInputChoiceSchema>,
}

/// Media source - an asset reference, a URL, or a value slot
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CreateMediaSourceSchema {
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

/// Poster configuration for a media item
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateMediaItemPosterSchema {
    /// Source for the poster image
    pub source: CreateMediaSourceSchema,
}

/// A single media item in the gallery
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateMediaItemSchema {
    /// Media type: "image", "video", "audio", "document", "file"
    pub media_kind: String,
    /// Source of the media content
    pub source: CreateMediaSourceSchema,
    /// Optional poster/thumbnail for video/audio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster: Option<CreateMediaItemPosterSchema>,
    /// Alt text fluent message for accessibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_text_fluent_message_id: Option<Uuid>,
    /// Caption fluent message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption_fluent_message_id: Option<Uuid>,
    /// Title fluent message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_fluent_message_id: Option<Uuid>,
}

/// Media subtype configuration
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBriefUnitMediaSchema {
    /// Media items in display order (must have at least 1 item)
    pub items: Vec<CreateMediaItemSchema>,
}

/// Subtype discriminated union for brief unit creation
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CreateBriefUnitSubtypeSchema {
    Markdown(CreateBriefUnitMarkdownSchema),
    Notice(CreateBriefUnitNoticeSchema),
    Input(CreateBriefUnitInputSchema),
    Media(CreateBriefUnitMediaSchema),
    None,
}

/// Create a single brief unit
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBriefUnitSchema {
    /// Unique key within the brief
    pub unit_key: String,
    /// Layout hints for rendering
    #[serde(default)]
    pub layout: serde_json::Value,
    /// Conditional visibility rules
    #[serde(default)]
    pub requires: serde_json::Value,
    /// Extension data
    #[serde(default)]
    pub extensions: serde_json::Value,
    /// The unit's subtype data
    pub subtype: CreateBriefUnitSubtypeSchema,
}

/// Batch create brief units
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateBriefUnitSetSchema {
    pub objs: Vec<CreateBriefUnitSchema>,
}
