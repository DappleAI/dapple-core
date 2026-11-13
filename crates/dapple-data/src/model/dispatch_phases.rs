use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Clone, Eq, PartialEq, Debug, Deserialize, Serialize, JsonSchema)]
pub struct DispatchPhase {
    pub code: String,
    pub terminal: bool,
    #[schemars(with = "String")]
    pub created_at: OffsetDateTime,
}
