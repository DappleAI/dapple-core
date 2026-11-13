use super::input_choices::{BriefUnitInputChoiceModel, InsertBriefUnitInputChoiceArgs};
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;

// Re-export public DTO from dapple-types
pub use dapple_types::model::brief_units::BriefUnitInput;

/// Internal model for brief unit input subtype
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BriefUnitInputModel {
    pub brief_unit_id: Uuid,
    pub control_type_key: String,
    pub label_text: Option<String>,
    pub label_fluent_message_id: Option<Uuid>,
    pub help_text: Option<String>,
    pub help_fluent_message_id: Option<Uuid>,
    pub placeholder_text: Option<String>,
    pub placeholder_fluent_message_id: Option<Uuid>,
    pub control_spec: serde_json::Value,
    pub choices: Vec<BriefUnitInputChoiceModel>,
}

impl From<BriefUnitInputModel> for BriefUnitInput {
    fn from(model: BriefUnitInputModel) -> Self {
        Self {
            control_type: model.control_type_key,
            label_text: model.label_text,
            label_fluent_message_id: model.label_fluent_message_id,
            help_text: model.help_text,
            help_fluent_message_id: model.help_fluent_message_id,
            placeholder_text: model.placeholder_text,
            placeholder_fluent_message_id: model.placeholder_fluent_message_id,
            control_spec: model.control_spec,
            choices: model.choices.into_iter().map(|c| c.into()).collect(),
        }
    }
}

/// Insert arguments for brief unit input subtype
#[derive(Clone, Debug)]
pub struct InsertBriefUnitInputArgs {
    pub control_type_key: String,
    pub label_text: Option<String>,
    pub label_fluent_message_id: Option<Uuid>,
    pub help_text: Option<String>,
    pub help_fluent_message_id: Option<Uuid>,
    pub placeholder_text: Option<String>,
    pub placeholder_fluent_message_id: Option<Uuid>,
    pub control_spec: serde_json::Value,
    pub choices: InsertBriefUnitInputChoiceArgs,
}
