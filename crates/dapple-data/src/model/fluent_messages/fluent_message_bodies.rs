use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// Re-export public DTO from dapple-types
pub use dapple_types::model::fluent_messages::FluentMessageLocalizedBody;

/// Internal model for fluent message localized body (from database)
/// Fields are Option because SQLx infers them as nullable from JOINs
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct FluentMessageBodyModel {
    pub id: Uuid,
    pub fluent_message_id: Uuid,
    pub locale_id: Option<i16>,
    pub locale_tag: Option<String>,
    pub body: Option<String>,
    pub created_at: Option<OffsetDateTime>,
}

impl From<FluentMessageBodyModel> for FluentMessageLocalizedBody {
    fn from(model: FluentMessageBodyModel) -> Self {
        Self {
            locale: model.locale_tag.unwrap_or_default(),
            body: model.body.unwrap_or_default(),
        }
    }
}

/// Column-wise insert arguments for localized bodies
#[derive(Clone, Debug)]
pub struct InsertFluentMessageBodyArgs {
    pub locale_tags: Vec<String>,
    pub bodies: Vec<String>,
}
