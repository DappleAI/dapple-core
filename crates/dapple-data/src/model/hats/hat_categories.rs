use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::hats::HatCategory;

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedHatCategoryModel {
    pub id: Uuid,
    pub hat_id: Uuid,
    pub category_id: Uuid,
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HatCategoryModel {
    pub id: Uuid,
    pub hat_id: Uuid,
    pub category_id: Uuid,
    pub created_at: OffsetDateTime,
}

impl From<InsertedHatCategoryModel> for HatCategoryModel {
    fn from(m: InsertedHatCategoryModel) -> Self {
        Self {
            id: m.id,
            hat_id: m.hat_id,
            category_id: m.category_id,
            created_at: m.created_at,
        }
    }
}

impl From<HatCategoryModel> for HatCategory {
    fn from(m: HatCategoryModel) -> Self {
        Self {
            id: m.id,
            hat_id: m.hat_id,
            category_id: m.category_id,
            created_at: m.created_at.into(),
        }
    }
}

impl_keyed!(InsertedHatCategoryModel, inserted);
impl_keyed!(HatCategoryModel);
