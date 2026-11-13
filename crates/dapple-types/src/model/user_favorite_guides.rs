use crate::DateTime;
use crate::model::guides::GuideRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Public DTO for a user's favorite guide.
/// Used for API responses.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct UserFavoriteGuide {
    pub guide: GuideRef,
    pub created_at: DateTime,
}
