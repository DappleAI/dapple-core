use crate::{DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for a canonical release pin.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct CanonicalReleasePin {
    pub id: Uuid,
    pub scope: String,
    pub environment: String,
    pub object_type: String,
    pub ref_key: String,
    pub version: String,
    pub resolved: bool,
    pub created_at: DateTime,
}

impl DataTransferObject for CanonicalReleasePin {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
