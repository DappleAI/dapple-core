use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct DispatchPhase {
    pub code: String,
    pub terminal: bool,
    #[schemars(with = "String")]
    pub created_at: OffsetDateTime,
}
