use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::brief_units::BriefUnit;

/// Typed display vocabulary for a brief output — the output-side analog of an
/// input's `control_type`. Closed and authored-controlled, so the wire form is a
/// real enum (the consumer's schema codegen emits a closed union) rather than a
/// free string. Authored as `:::brief-output { display: "..." }`.
///
/// The flat kinds render the bound value directly; `Hidden` marks a computed
/// slot that exists only for data flow (it must ride a `:::brief-output` to be
/// produced/bound) and is never shown to the operator.
#[derive(Clone, Copy, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BriefOutputDisplay {
    /// `<img>` from the asset content endpoint (value_kind: asset).
    Image,
    /// `<audio controls>` from the asset content endpoint (value_kind: asset).
    Audio,
    /// `<video controls>` from the asset content endpoint (value_kind: asset).
    Video,
    /// Plain text.
    #[default]
    Text,
    /// Rendered markdown.
    Markdown,
    /// Bulleted list (string[]).
    List,
    /// Tickable list (string[]); ticks are local UI only, never posted.
    Checklist,
    /// Columns = keys, one row per item (object[] of uniform scalar fields).
    Table,
    /// Label/value pairs (object {k: v}).
    Fields,
    /// Download card (icon + title + download anchor to the asset content
    /// endpoint) for document artifacts opened elsewhere — OTIO timelines,
    /// editable SVGs (value_kind: asset).
    File,
    /// Inline document viewer (`<iframe>` on the asset content endpoint, e.g.
    /// a PDF report from `document render`) plus a download link — the
    /// read-in-place sibling of `File` (value_kind: asset).
    Document,
    /// Data-flow slot: produced and bound but never rendered to the operator.
    Hidden,
}

impl BriefOutputDisplay {
    /// Every valid lookup key, in lockstep with the `rev.brief_output_display_hints`
    /// seed. The single source of truth for materialize-time validation.
    pub const ALL_KEYS: [&'static str; 12] = [
        "image",
        "audio",
        "video",
        "text",
        "markdown",
        "list",
        "checklist",
        "table",
        "fields",
        "file",
        "document",
        "hidden",
    ];

    /// Whether `key` is a recognized display hint.
    pub fn is_valid_key(key: &str) -> bool {
        Self::ALL_KEYS.contains(&key)
    }

    /// Parse a lookup-table key into the enum. Unknown / NULL keys fall back to
    /// [`BriefOutputDisplay::Text`] — the read-side default for an output with no
    /// authored `display`.
    pub fn from_key(key: &str) -> Self {
        match key {
            "image" => Self::Image,
            "audio" => Self::Audio,
            "video" => Self::Video,
            "markdown" => Self::Markdown,
            "list" => Self::List,
            "checklist" => Self::Checklist,
            "table" => Self::Table,
            "fields" => Self::Fields,
            "file" => Self::File,
            "document" => Self::Document,
            "hidden" => Self::Hidden,
            _ => Self::Text,
        }
    }
}

/// Public DTO for brief output
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefOutput {
    pub id: Uuid,
    pub unit_key: String,
    pub value_slot_id: Uuid,
    pub value_slot_name: String,
    pub source_kind: String,
    pub source_path: String,
    #[serde(default)]
    pub transform: serde_json::Value,
    /// How a client should render this output's bound value. Authored via
    /// `:::brief-output { display: "..." }`; defaults to `text`.
    #[serde(default)]
    pub display: BriefOutputDisplay,
    /// Collapse the rendered block behind a "Reveal" spoiler toggle.
    #[serde(default)]
    pub reveal: bool,
    /// Heading label for the block (hybrid: raw fallback + Fluent reference).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_fluent_message_id: Option<Uuid>,
    /// Sibling-format grouping: outputs sharing a `display_group` are the same
    /// deliverable in different formats. Opaque author-chosen token; `None` ⇒
    /// ungrouped. See [`crate::model::journeys::ExposedSlotDisplay::group`] for
    /// the render contract (hidden siblings are filtered before grouping; a
    /// group of one renders as an ordinary output).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_group: Option<String>,
    /// Short label naming this output's format within its group (`"html"`,
    /// `"pdf"`, …). Open vocabulary — an author may coin one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_format: Option<String>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

/// Public DTO for brief
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct Brief {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub units: Vec<BriefUnit>,
    pub outputs: Vec<BriefOutput>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

/// Public DTO for brief path
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BriefPath {
    pub id: Uuid,
    pub name: String,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl DataTransferObject for Brief {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl DataTransferObject for BriefPath {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_default_is_text() {
        assert_eq!(BriefOutputDisplay::default(), BriefOutputDisplay::Text);
    }

    #[test]
    fn display_from_key_roundtrips_and_falls_back() {
        for key in BriefOutputDisplay::ALL_KEYS {
            assert!(BriefOutputDisplay::is_valid_key(key));
        }
        assert_eq!(
            BriefOutputDisplay::from_key("checklist"),
            BriefOutputDisplay::Checklist
        );
        assert_eq!(
            BriefOutputDisplay::from_key("hidden"),
            BriefOutputDisplay::Hidden
        );
        // Unknown / NULL keys fall back to Text.
        assert_eq!(
            BriefOutputDisplay::from_key("bogus"),
            BriefOutputDisplay::Text
        );
        assert!(!BriefOutputDisplay::is_valid_key("bogus"));

        // Regression guard for the media hints. A binary that dropped `video` from
        // ALL_KEYS silently downgraded a guide's `display: "video"` to text, and
        // content-hash dedup then froze the stale row so re-syncs never fixed it.
        // Assert them explicitly (the ALL_KEYS loop above can't catch a REMOVED key).
        assert!(BriefOutputDisplay::is_valid_key("video"));
        assert!(BriefOutputDisplay::is_valid_key("audio"));
        assert_eq!(
            BriefOutputDisplay::from_key("video"),
            BriefOutputDisplay::Video
        );
        assert_eq!(
            BriefOutputDisplay::from_key("audio"),
            BriefOutputDisplay::Audio
        );
        assert!(BriefOutputDisplay::is_valid_key("file"));
        assert_eq!(
            BriefOutputDisplay::from_key("file"),
            BriefOutputDisplay::File
        );
        assert!(BriefOutputDisplay::is_valid_key("document"));
        assert_eq!(
            BriefOutputDisplay::from_key("document"),
            BriefOutputDisplay::Document
        );
    }

    #[test]
    fn display_serializes_snake_case() {
        // The wire form the UI codegen consumes.
        assert_eq!(
            serde_json::to_string(&BriefOutputDisplay::Checklist).unwrap(),
            "\"checklist\""
        );
        let parsed: BriefOutputDisplay = serde_json::from_str("\"table\"").unwrap();
        assert_eq!(parsed, BriefOutputDisplay::Table);
    }
}
