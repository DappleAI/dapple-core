use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;

// Re-export public DTO from dapple-types
pub use dapple_types::model::brief_units::BriefUnitNotice;

/// Internal model for brief unit notice subtype
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefUnitNoticeModel {
    pub brief_unit_id: Uuid,
    pub level: String,
    pub title_text: Option<String>,
    pub title_fluent_message_id: Option<Uuid>,
    pub detail_text: Option<String>,
    pub detail_fluent_message_id: Option<Uuid>,
}

impl From<BriefUnitNoticeModel> for BriefUnitNotice {
    fn from(model: BriefUnitNoticeModel) -> Self {
        Self {
            level: model.level,
            title_text: model.title_text,
            title_fluent_message_id: model.title_fluent_message_id,
            detail_text: model.detail_text,
            detail_fluent_message_id: model.detail_fluent_message_id,
        }
    }
}

/// Insert arguments for brief unit notice subtype
#[derive(Clone, Debug)]
pub struct InsertBriefUnitNoticeArgs {
    pub level: String,
    pub title_text: Option<String>,
    pub title_fluent_message_id: Option<Uuid>,
    pub detail_text: Option<String>,
    pub detail_fluent_message_id: Option<Uuid>,
}
