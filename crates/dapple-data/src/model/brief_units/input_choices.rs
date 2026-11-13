use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::brief_units::BriefUnitInputChoice;

/// Internal model for brief unit input choice
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefUnitInputChoiceModel {
    pub id: Uuid,
    pub brief_unit_id: Uuid,
    pub label_text: Option<String>,
    pub label_fluent_message_id: Option<Uuid>,
    pub value_text: String,
    pub created_at: OffsetDateTime,
}

/// Result from INSERT...RETURNING for brief unit input choice
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedBriefUnitInputChoiceModel {
    pub id: Uuid,
    pub brief_unit_id: Uuid,
    pub label_text: Option<String>,
    pub label_fluent_message_id: Option<Uuid>,
    pub value_text: String,
    pub created_at: OffsetDateTime,
}

impl From<BriefUnitInputChoiceModel> for BriefUnitInputChoice {
    fn from(model: BriefUnitInputChoiceModel) -> Self {
        Self {
            id: model.id,
            label_text: model.label_text,
            label_fluent_message_id: model.label_fluent_message_id,
            value_text: model.value_text,
            created_at: model.created_at.into(),
        }
    }
}

/// Column-wise insert arguments for brief unit input choices
#[derive(Clone, Debug, Default)]
pub struct InsertBriefUnitInputChoiceArgs {
    pub label_texts: Vec<Option<String>>,
    pub label_fluent_message_ids: Vec<Option<Uuid>>,
    pub value_texts: Vec<String>,
}
