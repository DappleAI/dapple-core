use crate::Keyed;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::playbooks::PlaybookCategory;

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedPlaybookCategoryModel {
    pub id: Uuid,
    pub playbook_id: Uuid,
    pub category_id: Uuid,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PlaybookCategoryModel {
    pub id: Uuid,
    pub playbook_id: Uuid,
    pub category_id: Uuid,
    pub created_at: OffsetDateTime,
}

impl From<InsertedPlaybookCategoryModel> for PlaybookCategoryModel {
    fn from(m: InsertedPlaybookCategoryModel) -> Self {
        Self {
            id: m.id,
            playbook_id: m.playbook_id,
            category_id: m.category_id,
            created_at: m.created_at,
        }
    }
}

impl From<PlaybookCategoryModel> for PlaybookCategory {
    fn from(model: PlaybookCategoryModel) -> Self {
        Self {
            id: model.id,
            playbook_id: model.playbook_id,
            category_id: model.category_id,
            created_at: model.created_at.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertPlaybookCategoryArgs {
    pub playbook_category_ids: Vec<Uuid>,
}

impl crate::traits::InsertedModel for InsertedPlaybookCategoryModel {}
impl Keyed for InsertedPlaybookCategoryModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
impl crate::traits::RelationalObject for InsertedPlaybookCategoryModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl Keyed for PlaybookCategoryModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
impl crate::traits::RelationalObject for PlaybookCategoryModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
