pub mod input;
pub mod input_choices;
pub mod markdown;
pub mod media;
pub mod notice;

use crate::error::DataError;
use input::{BriefUnitInputModel, InsertBriefUnitInputArgs};
use input_choices::BriefUnitInputChoiceModel;
use markdown::{BriefUnitMarkdownModel, InsertBriefUnitMarkdownArgs};
use media::{
    BriefUnitMediaItemModel, BriefUnitMediaModel, FlatBriefUnitMediaItemModel,
    InsertBriefUnitMediaArgs, fold_media_items,
};
use notice::{BriefUnitNoticeModel, InsertBriefUnitNoticeArgs};
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::brief_units::*;

// ============================================================================
// BriefUnit (owned dependent of Brief)
// ============================================================================

/// Result from INSERT...RETURNING for brief unit
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedBriefUnitModel {
    pub id: Uuid,
    pub brief_id: Uuid,
    pub unit_key: String,
    pub layout: serde_json::Value,
    pub requires: serde_json::Value,
    pub extensions: serde_json::Value,
    pub created_at: OffsetDateTime,
}

/// Flat result from SQL query (unit + 1:1 subtypes, one row per unit).
/// Matches columns returned by `select_brief_units_by_brief_ids.sql`.
/// Choice and media item rows are provided separately.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatBriefUnitModel {
    // brief_units fields
    pub id: Uuid,
    pub brief_id: Uuid,
    pub unit_key: String,
    pub layout: serde_json::Value,
    pub requires: serde_json::Value,
    pub extensions: serde_json::Value,
    pub created_at: OffsetDateTime,

    // brief_unit_markdown fields (nullable - 1:1)
    pub markdown_text: Option<String>,
    pub markdown_fluent_message_id: Option<Uuid>,
    pub markdown_format: Option<String>,
    pub markdown_deliverable_spec: Option<serde_json::Value>,

    // brief_unit_notice fields (nullable - 1:1)
    pub notice_level: Option<String>,
    pub notice_title_text: Option<String>,
    pub notice_title_fluent_message_id: Option<Uuid>,
    pub notice_detail_text: Option<String>,
    pub notice_detail_fluent_message_id: Option<Uuid>,

    // brief_unit_input fields (nullable - 1:1)
    pub input_control_type_id: Option<i16>,
    pub input_control_type_key: Option<String>,
    pub input_label_text: Option<String>,
    pub input_label_fluent_message_id: Option<Uuid>,
    pub input_help_text: Option<String>,
    pub input_help_fluent_message_id: Option<Uuid>,
    pub input_placeholder_text: Option<String>,
    pub input_placeholder_fluent_message_id: Option<Uuid>,
    pub input_control_spec: Option<serde_json::Value>,

    // brief_unit_media marker (nullable - presence indicates media subtype)
    pub media_created_at: Option<OffsetDateTime>,
}

impl FlatBriefUnitModel {
    /// Fold flat unit rows into structured `BriefUnitModel` instances.
    ///
    /// Unlike the old implementation that required deduplication across a
    /// massive JOIN, each row here represents exactly one unit (1:1 subtypes
    /// are inlined). Choice rows and media item rows are provided separately
    /// and matched by `brief_unit_id`.
    pub fn fold_models(
        models: Vec<Self>,
        choices: Vec<BriefUnitInputChoiceModel>,
        media_items: Vec<FlatBriefUnitMediaItemModel>,
    ) -> Result<Vec<BriefUnitModel>, DataError> {
        if models.is_empty() {
            return Ok(Vec::new());
        }

        // Pre-fold media items into structured models
        let folded_media_items: Vec<BriefUnitMediaItemModel> = fold_media_items(media_items);

        let units: Vec<BriefUnitModel> = models
            .into_iter()
            .map(|m| {
                // Determine subtype by checking which 1:1 fields are populated
                let subtype = if m.notice_level.is_some() {
                    BriefUnitSubtypeModel::Notice(BriefUnitNoticeModel {
                        brief_unit_id: m.id,
                        level: m.notice_level.clone().unwrap_or_default(),
                        title_text: m.notice_title_text.clone(),
                        title_fluent_message_id: m.notice_title_fluent_message_id,
                        detail_text: m.notice_detail_text.clone(),
                        detail_fluent_message_id: m.notice_detail_fluent_message_id,
                    })
                } else if m.input_control_type_key.is_some() {
                    // Collect choices for this unit
                    let unit_choices: Vec<BriefUnitInputChoiceModel> = choices
                        .iter()
                        .filter(|c| c.brief_unit_id == m.id)
                        .cloned()
                        .collect();

                    BriefUnitSubtypeModel::Input(BriefUnitInputModel {
                        brief_unit_id: m.id,
                        control_type_key: m.input_control_type_key.clone().unwrap(),
                        label_text: m.input_label_text.clone(),
                        label_fluent_message_id: m.input_label_fluent_message_id,
                        help_text: m.input_help_text.clone(),
                        help_fluent_message_id: m.input_help_fluent_message_id,
                        placeholder_text: m.input_placeholder_text.clone(),
                        placeholder_fluent_message_id: m.input_placeholder_fluent_message_id,
                        control_spec: m.input_control_spec.clone().unwrap_or_default(),
                        choices: unit_choices,
                    })
                } else if m.media_created_at.is_some() {
                    // Collect media items for this unit
                    let items: Vec<BriefUnitMediaItemModel> = folded_media_items
                        .iter()
                        .filter(|mi| mi.brief_unit_id == m.id)
                        .cloned()
                        .collect();
                    BriefUnitSubtypeModel::Media(BriefUnitMediaModel {
                        brief_unit_id: m.id,
                        items,
                    })
                } else if m.markdown_text.is_some() || m.markdown_fluent_message_id.is_some() {
                    BriefUnitSubtypeModel::Markdown(BriefUnitMarkdownModel {
                        brief_unit_id: m.id,
                        markdown_text: m.markdown_text.clone(),
                        fluent_message_id: m.markdown_fluent_message_id,
                        format: m.markdown_format.clone(),
                        deliverable_spec: m.markdown_deliverable_spec.clone().unwrap_or_default(),
                    })
                } else {
                    BriefUnitSubtypeModel::None
                };

                BriefUnitModel {
                    id: m.id,
                    brief_id: Uuid::nil(), // Not available from SELECT; set by caller if needed
                    unit_key: m.unit_key,
                    layout: m.layout,
                    requires: m.requires,
                    extensions: m.extensions,
                    created_at: m.created_at,
                    subtype,
                }
            })
            .collect();

        Ok(units)
    }
}

/// Internal model for brief unit with all children
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefUnitModel {
    pub id: Uuid,
    pub brief_id: Uuid,
    pub unit_key: String,
    pub layout: serde_json::Value,
    pub requires: serde_json::Value,
    pub extensions: serde_json::Value,
    pub created_at: OffsetDateTime,
    pub subtype: BriefUnitSubtypeModel,
}

/// Internal enum for brief unit subtype
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum BriefUnitSubtypeModel {
    Markdown(BriefUnitMarkdownModel),
    Notice(BriefUnitNoticeModel),
    Input(BriefUnitInputModel),
    Media(BriefUnitMediaModel),
    None, // Unit with no UI subtype
}

impl From<BriefUnitModel> for BriefUnit {
    fn from(model: BriefUnitModel) -> Self {
        Self {
            id: model.id,
            unit_key: model.unit_key,
            layout: model.layout,
            requires: model.requires,
            extensions: model.extensions,
            subtype: model.subtype.into(),
            created_at: model.created_at.into(),
        }
    }
}

impl From<BriefUnitSubtypeModel> for BriefUnitSubtype {
    fn from(model: BriefUnitSubtypeModel) -> Self {
        match model {
            BriefUnitSubtypeModel::Markdown(m) => BriefUnitSubtype::Markdown(m.into()),
            BriefUnitSubtypeModel::Notice(n) => BriefUnitSubtype::Notice(n.into()),
            BriefUnitSubtypeModel::Input(i) => BriefUnitSubtype::Input(i.into()),
            BriefUnitSubtypeModel::Media(m) => BriefUnitSubtype::Media(m.into()),
            BriefUnitSubtypeModel::None => BriefUnitSubtype::None,
        }
    }
}

impl crate::Keyed for BriefUnit {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

// ============================================================================
// Insert Args
// ============================================================================

/// Column-wise insert arguments for brief units
#[derive(Clone, Debug)]
pub struct InsertBriefUnitArgs {
    pub brief_ids: Vec<Uuid>,
    pub unit_keys: Vec<String>,
    pub layouts: Vec<serde_json::Value>,
    pub requires: Vec<serde_json::Value>,
    pub extensions: Vec<serde_json::Value>,
    /// 1-based authored position of each unit within its OWN brief's unit
    /// sequence — shared numbering with
    /// [`super::briefs::outputs::InsertBriefOutputArgs::ords`], since both are
    /// assigned from one walk of `brief_def.unit_names`. NOT the array index:
    /// this batch spans several briefs and excludes their outputs. `0` means
    /// "no authored position" (the pre-migration-95 sentinel).
    pub ords: Vec<i32>,
    // Nested args for children (one entry per unit)
    pub subtypes: Vec<InsertBriefUnitSubtypeArgs>,
}

/// Insert args for subtype (one per unit)
#[derive(Clone, Debug)]
pub enum InsertBriefUnitSubtypeArgs {
    Markdown(InsertBriefUnitMarkdownArgs),
    Notice(InsertBriefUnitNoticeArgs),
    Input(InsertBriefUnitInputArgs),
    Media(InsertBriefUnitMediaArgs),
    None,
}

/// Internal ID lookup result
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefUnitInternalId {
    pub id: Uuid,
}
