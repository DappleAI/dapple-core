use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::briefs::{BriefOutput, BriefOutputDisplay};

/// Internal model for brief output (from SELECT query)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefOutputModel {
    pub id: Uuid,
    pub brief_id: Uuid,
    pub unit_key: String,
    pub value_slot_id: Uuid,
    pub value_slot_name: String,
    pub source_kind: String,
    pub source_path: String,
    pub transform: serde_json::Value,
    /// Display-hint lookup key (COALESCE'd to `text` in the SELECT, so always
    /// present even when the output has no authored display).
    pub display_hint: String,
    pub reveal: bool,
    pub title_text: Option<String>,
    pub title_fluent_message_id: Option<Uuid>,
    /// Sibling-format grouping token (opaque, author-chosen). `None` ⇒ ungrouped.
    pub display_group: Option<String>,
    /// Format label within the group (`html`, `pdf`, …). Open vocabulary.
    pub display_format: Option<String>,
    pub created_at: OffsetDateTime,
}

/// Result from INSERT...RETURNING for brief output
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedBriefOutputModel {
    pub id: Uuid,
    pub brief_id: Uuid,
    pub unit_key: String,
    pub value_slot_id: Uuid,
    pub value_slot_name: String,
    pub source_kind_key: String,
    pub source_path: String,
    pub transform: serde_json::Value,
    pub display_hint: String,
    pub reveal: bool,
    pub title_text: Option<String>,
    pub title_fluent_message_id: Option<Uuid>,
    pub display_group: Option<String>,
    pub display_format: Option<String>,
    pub created_at: OffsetDateTime,
}

impl From<BriefOutputModel> for BriefOutput {
    fn from(model: BriefOutputModel) -> Self {
        Self {
            id: model.id,
            unit_key: model.unit_key,
            value_slot_id: model.value_slot_id,
            value_slot_name: model.value_slot_name,
            source_kind: model.source_kind,
            source_path: model.source_path,
            transform: model.transform,
            display: BriefOutputDisplay::from_key(&model.display_hint),
            reveal: model.reveal,
            title_text: model.title_text,
            title_fluent_message_id: model.title_fluent_message_id,
            display_group: model.display_group,
            display_format: model.display_format,
            created_at: model.created_at.into(),
        }
    }
}

/// Column-wise insert arguments for brief outputs.
///
/// All vectors MUST be the same length. `display_hint_keys` carry a valid lookup
/// key per output (default `text`); the modifiers (`reveals`, `title_texts`,
/// `title_fluent_message_ids`, `display_groups`, `display_formats`) are parallel.
#[derive(Clone, Debug)]
pub struct InsertBriefOutputArgs {
    pub brief_ids: Vec<Uuid>,
    pub value_slot_ids: Vec<Uuid>,
    pub unit_keys: Vec<String>,
    pub source_kind_keys: Vec<String>,
    pub source_paths: Vec<String>,
    pub transforms: Vec<serde_json::Value>,
    pub display_hint_keys: Vec<String>,
    pub reveals: Vec<bool>,
    pub title_texts: Vec<Option<String>>,
    pub title_fluent_message_ids: Vec<Option<Uuid>>,
    pub display_groups: Vec<Option<String>>,
    pub display_formats: Vec<Option<String>>,
    /// 1-based authored position of each output within its OWN brief's unit
    /// sequence — shared numbering with [`super::super::brief_units::InsertBriefUnitArgs::ords`],
    /// since both are assigned from one walk of `brief_def.unit_names`. NOT the
    /// array index: this batch spans several briefs and holds only the outputs.
    /// `0` means "no authored position" (the pre-migration-95 sentinel).
    pub ords: Vec<i32>,
}
