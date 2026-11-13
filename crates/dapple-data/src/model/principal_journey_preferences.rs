//! Per-principal preferences overlaying a journey.
//!
//! `rev.journeys` rows are immutable session-instances of a playbook. Each
//! viewer (principal) can attach their own display preferences — a name
//! override and a soft-hide timestamp — by appending rows to
//! `rev.principal_journey_preferences`. Reads resolve the latest row per
//! `(principal_id, journey_id)` via `DISTINCT ON ... ORDER BY ..., id DESC`.
//!
//! The journey-read queries in [`crate::query::journeys`] LEFT-JOIN against
//! this table and surface `COALESCE(prefs.name, journeys.name)` plus
//! `prefs.hidden_at`. List queries additionally filter rows whose
//! `prefs.hidden_at` is non-NULL so a principal's hidden journeys disappear
//! from their own list views without affecting any other principal's view.

use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

/// Result row from the single-row insert.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedPrincipalJourneyPreferenceModel {
    pub id: Uuid,
    pub principal_id: Uuid,
    pub journey_id: Uuid,
    pub name: Option<String>,
    pub hidden_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}
