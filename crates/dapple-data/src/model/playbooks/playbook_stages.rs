use crate::Keyed;
use crate::MergeOrdered;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::playbooks::PlaybookStage;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertPlaybookStageArgs {
    pub name: String,
    /// Nullable fallback text (direct creation). Guide-ingest leaves this `None`
    /// and supplies `description_fluent_message_id` instead.
    #[serde(default)]
    pub description: Option<String>,
    /// Exact-version pin to the stage's display Fluent message (guide-ingest).
    #[serde(default)]
    pub description_fluent_message_id: Option<Uuid>,
    /// Authored stage-kind key (`:::stage { kind: "..." }`). Raw authored
    /// value; the insert's LEFT JOIN stores NULL for unknown keys.
    #[serde(default)]
    pub kind: Option<String>,
    /// Authored icon names (`:::stage { icon: [...] }`), in authored order.
    /// An OPEN vocabulary — stored verbatim, never validated here or at the
    /// insert. Empty ⇒ stored NULL, matching the empty-omit hash fold.
    #[serde(default)]
    pub icon: Vec<String>,
    /// Resolved step IDs that belong to this stage (for stage_steps creation).
    pub step_ids: Vec<Uuid>,
    /// If set, reuse this existing stage id instead of creating a new one.
    #[serde(default)]
    pub stage_id: Option<Uuid>,
    /// Content hash for new stages (ignored if stage_id is set).
    #[serde(default)]
    pub content_hash: Option<String>,
    /// If true, this stage is reused as-is (same content hash) and already has
    /// its stage_steps. Skip creating stage_steps in assemble_playbook_children.
    #[serde(default)]
    pub reused: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedPlaybookStageModel {
    pub id: Uuid,
    pub playbook_id: Uuid,
    pub stage_id: Uuid,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PlaybookStageModel {
    pub id: Uuid,
    pub playbook_id: Uuid,
    pub stage_id: Uuid,
    pub created_at: OffsetDateTime,
    pub steps: Vec<crate::model::playbooks::playbook_steps::PlaybookStepModel>,
}

impl From<InsertedPlaybookStageModel> for PlaybookStageModel {
    fn from(model: InsertedPlaybookStageModel) -> Self {
        Self {
            id: model.id,
            playbook_id: model.playbook_id,
            stage_id: model.stage_id,
            created_at: model.created_at,
            steps: vec![],
        }
    }
}

impl Keyed for InsertedPlaybookStageModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::InsertedModel for InsertedPlaybookStageModel {}

impl crate::traits::RelationalObject for InsertedPlaybookStageModel {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for PlaybookStageModel {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}

impl crate::traits::OwnedDependent for PlaybookStageModel {
    fn parent_id(&self) -> &sqlx::types::Uuid {
        &self.playbook_id
    }
}

impl crate::traits::OwnedDependent for InsertedPlaybookStageModel {
    fn parent_id(&self) -> &sqlx::types::Uuid {
        &self.playbook_id
    }
}

impl Keyed for PlaybookStageModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl From<PlaybookStageModel> for PlaybookStage {
    fn from(model: PlaybookStageModel) -> Self {
        Self {
            id: model.id,
            playbook_id: model.playbook_id,
            steps: model.steps.into_iter().map(|s| s.into()).collect(),
            created_at: model.created_at.into(),
        }
    }
}

impl MergeOrdered for PlaybookStageModel {
    type Parent = InsertedPlaybookStageModel;
    type ChildRow = crate::model::playbooks::playbook_steps::PlaybookStepModel;
    type Child = crate::model::playbooks::playbook_steps::PlaybookStepModel;
    type Final = PlaybookStageModel;
    type Error = crate::error::DataError;

    #[inline]
    fn parent_id(p: &Self::Parent) -> &Uuid {
        &p.id
    }

    #[inline]
    fn child_id(c: &Self::ChildRow) -> &Uuid {
        &c.playbook_stage_id
    }

    #[inline]
    fn finalize(parent: &Self::Parent, children: Vec<Self::Child>) -> Self::Final {
        PlaybookStageModel {
            id: parent.id,
            playbook_id: parent.playbook_id,
            stage_id: parent.stage_id,
            created_at: parent.created_at,
            steps: children,
        }
    }
}
