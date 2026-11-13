use crate::{DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for user detail
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct UserDetail {
    pub id: Uuid,
    pub email: String,
    pub created_at: Option<DateTime>,
}

impl DataTransferObject for UserDetail {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
