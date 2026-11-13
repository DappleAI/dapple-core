use crate::DateTime;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// ============================================================================
// Visibility Enum
// ============================================================================

/// Visibility options for overlay entries
#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Expose,
    Mask,
}

impl Visibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Expose => "expose",
            Visibility::Mask => "mask",
        }
    }
}

impl TryFrom<&str> for Visibility {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "expose" => Ok(Visibility::Expose),
            "mask" => Ok(Visibility::Mask),
            _ => Err(format!("Invalid visibility: {}", s)),
        }
    }
}

impl From<String> for Visibility {
    fn from(s: String) -> Self {
        Visibility::try_from(s.as_str()).unwrap_or(Visibility::Mask)
    }
}

// ============================================================================
// Playbook Stage Overlays
// ============================================================================

/// Result from INSERT...RETURNING for playbook stage overlay
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedPlaybookStageOverlayModel {
    pub id: Uuid,
    pub playbook_stage_id: Uuid,
    pub value_slot_path_id: Uuid,
    pub visibility: String,
    pub created_at: OffsetDateTime,
}

/// Internal model for playbook stage overlay
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PlaybookStageOverlayModel {
    pub id: Uuid,
    pub playbook_stage_id: Uuid,
    pub value_slot_path_id: Uuid,
    pub visibility: Visibility,
    pub created_at: OffsetDateTime,
}

impl From<InsertedPlaybookStageOverlayModel> for PlaybookStageOverlayModel {
    fn from(model: InsertedPlaybookStageOverlayModel) -> Self {
        Self {
            id: model.id,
            playbook_stage_id: model.playbook_stage_id,
            value_slot_path_id: model.value_slot_path_id,
            visibility: model.visibility.into(),
            created_at: model.created_at,
        }
    }
}

/// Public DTO for playbook stage overlay
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct PlaybookStageOverlay {
    pub id: Uuid,
    pub playbook_stage_id: Uuid,
    pub value_slot_path_id: Uuid,
    pub visibility: Visibility,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl From<PlaybookStageOverlayModel> for PlaybookStageOverlay {
    fn from(model: PlaybookStageOverlayModel) -> Self {
        Self {
            id: model.id,
            playbook_stage_id: model.playbook_stage_id,
            value_slot_path_id: model.value_slot_path_id,
            visibility: model.visibility,
            created_at: model.created_at.into(),
        }
    }
}

impl From<InsertedPlaybookStageOverlayModel> for PlaybookStageOverlay {
    fn from(model: InsertedPlaybookStageOverlayModel) -> Self {
        let model: PlaybookStageOverlayModel = model.into();
        model.into()
    }
}

/// Column-wise insert arguments for playbook stage overlays
#[derive(Clone, Debug)]
pub struct InsertPlaybookStageOverlaysArgs {
    pub playbook_stage_ids: Vec<Uuid>,
    pub value_slot_path_ids: Vec<Uuid>,
    pub visibilities: Vec<String>,
}

// ============================================================================
// Stage Step Overlays (formerly Playbook Step Overlays)
// ============================================================================

/// Result from INSERT...RETURNING for stage step overlay
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedStageStepOverlayModel {
    pub id: Uuid,
    pub stage_step_id: Uuid,
    pub value_slot_path_id: Uuid,
    pub visibility: String,
    pub created_at: OffsetDateTime,
}

/// Internal model for stage step overlay
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StageStepOverlayModel {
    pub id: Uuid,
    pub stage_step_id: Uuid,
    pub value_slot_path_id: Uuid,
    pub visibility: Visibility,
    pub created_at: OffsetDateTime,
}

impl From<InsertedStageStepOverlayModel> for StageStepOverlayModel {
    fn from(model: InsertedStageStepOverlayModel) -> Self {
        Self {
            id: model.id,
            stage_step_id: model.stage_step_id,
            value_slot_path_id: model.value_slot_path_id,
            visibility: model.visibility.into(),
            created_at: model.created_at,
        }
    }
}

/// Public DTO for stage step overlay
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct StageStepOverlay {
    pub id: Uuid,
    pub stage_step_id: Uuid,
    pub value_slot_path_id: Uuid,
    pub visibility: Visibility,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl From<StageStepOverlayModel> for StageStepOverlay {
    fn from(model: StageStepOverlayModel) -> Self {
        Self {
            id: model.id,
            stage_step_id: model.stage_step_id,
            value_slot_path_id: model.value_slot_path_id,
            visibility: model.visibility,
            created_at: model.created_at.into(),
        }
    }
}

impl From<InsertedStageStepOverlayModel> for StageStepOverlay {
    fn from(model: InsertedStageStepOverlayModel) -> Self {
        let model: StageStepOverlayModel = model.into();
        model.into()
    }
}

/// Column-wise insert arguments for stage step overlays
#[derive(Clone, Debug)]
pub struct InsertStageStepOverlaysArgs {
    pub stage_step_ids: Vec<Uuid>,
    pub value_slot_path_ids: Vec<Uuid>,
    pub visibilities: Vec<String>,
}

// ============================================================================
// Step Task Overlays (formerly Playbook Task Overlays)
// ============================================================================

/// Result from INSERT...RETURNING for step task overlay
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedStepTaskOverlayModel {
    pub id: Uuid,
    pub step_task_id: Uuid,
    pub value_slot_path_id: Uuid,
    pub visibility: String,
    pub created_at: OffsetDateTime,
}

/// Internal model for step task overlay
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StepTaskOverlayModel {
    pub id: Uuid,
    pub step_task_id: Uuid,
    pub value_slot_path_id: Uuid,
    pub visibility: Visibility,
    pub created_at: OffsetDateTime,
}

impl From<InsertedStepTaskOverlayModel> for StepTaskOverlayModel {
    fn from(model: InsertedStepTaskOverlayModel) -> Self {
        Self {
            id: model.id,
            step_task_id: model.step_task_id,
            value_slot_path_id: model.value_slot_path_id,
            visibility: model.visibility.into(),
            created_at: model.created_at,
        }
    }
}

/// Public DTO for step task overlay
#[derive(Clone, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct StepTaskOverlay {
    pub id: Uuid,
    pub step_task_id: Uuid,
    pub value_slot_path_id: Uuid,
    pub visibility: Visibility,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl From<StepTaskOverlayModel> for StepTaskOverlay {
    fn from(model: StepTaskOverlayModel) -> Self {
        Self {
            id: model.id,
            step_task_id: model.step_task_id,
            value_slot_path_id: model.value_slot_path_id,
            visibility: model.visibility,
            created_at: model.created_at.into(),
        }
    }
}

impl From<InsertedStepTaskOverlayModel> for StepTaskOverlay {
    fn from(model: InsertedStepTaskOverlayModel) -> Self {
        let model: StepTaskOverlayModel = model.into();
        model.into()
    }
}

/// Column-wise insert arguments for step task overlays
#[derive(Clone, Debug)]
pub struct InsertStepTaskOverlaysArgs {
    pub step_task_ids: Vec<Uuid>,
    pub value_slot_path_ids: Vec<Uuid>,
    pub visibilities: Vec<String>,
}

// ============================================================================
// Visible Value Slot Result
// ============================================================================

impl crate::traits::DataTransferObject for PlaybookStageOverlay {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::DataTransferObject for StageStepOverlay {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::DataTransferObject for StepTaskOverlay {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

/// Result from overlay resolution query - visible value slots for a task
/// Note: Fields are Option because SQL UNION makes columns nullable
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VisibleValueSlotPathModel {
    pub id: Option<Uuid>,
    pub name: Option<String>,
    pub value_kind_id: Option<i16>,
    pub value_kind_key: Option<String>,
}
