//! Insert per-principal journey preferences. Reads happen inline in the
//! journey-select queries (see [`crate::query::journeys`]) via a `viewer_prefs`
//! CTE that picks the latest row per `(principal_id, journey_id)`.

use crate::DbPool;
use crate::error::DataError;
use crate::model::principal_journey_preferences::InsertedPrincipalJourneyPreferenceModel;
use sqlx::types::Uuid;
use time::OffsetDateTime;

/// Append a single preference row for `(principal_id, journey_id)`. PATCH on
/// the journey endpoint passes `name = Some(...)` and `hidden_at = None`;
/// DELETE passes `name = None` and `hidden_at = Some(now)`. Both are NULL on
/// the row that the principal hasn't touched yet; the journey-select queries
/// fall through to `rev.journeys.name` when `prefs.name` is NULL.
pub async fn insert_principal_journey_preference(
    pool: &DbPool,
    principal_id: Uuid,
    journey_id: Uuid,
    name: Option<String>,
    hidden_at: Option<OffsetDateTime>,
) -> Result<InsertedPrincipalJourneyPreferenceModel, DataError> {
    let row = sqlx::query_file_as!(
        InsertedPrincipalJourneyPreferenceModel,
        "assets/.sql/principal_journey_preferences/insert_principal_journey_preference.sql",
        hidden_at,
        journey_id,
        name,
        principal_id,
    )
    .fetch_one(pool.get_pool())
    .await?;

    Ok(row)
}
