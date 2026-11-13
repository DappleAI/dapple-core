use dapple_types::model::guides::GuidePlaybookRef;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

/// Database model for the rev.guide_playbooks junction table.
#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct GuidePlaybookModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub playbook_id: Uuid,
    pub chapter_index: i16,
    pub created_at: OffsetDateTime,
}

/// Enriched model returned from the batch query joining through
/// versioned_objects → object_paths → playbook_path_details.
#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct GuidePlaybookEnrichedModel {
    pub guide_id: Uuid,
    pub playbook_id: Uuid,
    pub playbook_path_id: Uuid,
    pub playbook_slug: String,
    pub playbook_name: String,
    pub chapter_index: i16,
}

impl From<GuidePlaybookEnrichedModel> for GuidePlaybookRef {
    fn from(m: GuidePlaybookEnrichedModel) -> Self {
        Self {
            playbook_id: m.playbook_id,
            playbook_path_id: m.playbook_path_id,
            playbook_slug: m.playbook_slug,
            playbook_name: m.playbook_name,
            chapter_index: m.chapter_index,
        }
    }
}
