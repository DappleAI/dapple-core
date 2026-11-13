use crate::Keyed;
use crate::error::DataError;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::user_details::*;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatUserDetailModel {
    pub id: Uuid,
    pub email: Option<String>,
    pub created_at: OffsetDateTime,
}

impl FlatUserDetailModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<UserDetailModel>, DataError> {
        Ok(models
            .into_iter()
            .map(|m| UserDetailModel {
                id: m.id,
                email: m.email,
                created_at: m.created_at,
            })
            .collect())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserDetailModel {
    pub id: Uuid,
    pub email: Option<String>,
    pub created_at: OffsetDateTime,
}

impl Keyed for UserDetailModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for UserDetailModel {
    fn id(&self) -> &Uuid {
        <Self as crate::Keyed>::id(self)
    }
}

impl crate::traits::SharedDependent for UserDetailModel {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}

impl From<UserDetailModel> for UserDetail {
    fn from(model: UserDetailModel) -> Self {
        Self {
            id: model.id,
            //TODO: make this NOT NULL in the DB to avoid the expect()
            email: model.email.expect("email should be present in UserDetail"),
            created_at: None, // select() does not include created_at in current SQL; leave None
        }
    }
}
