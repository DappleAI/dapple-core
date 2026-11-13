//! Build `JourneyBindingsResponse` from the data layer.
//!
//! Used by both the internal (`dapple-prompt-api`) and external
//! (`dapple-prompt-external-api`) `GET /api/journeys/{id}/bindings`
//! handlers. The handlers add their own response framing (status, headers,
//! namespace gating) on top of this pure builder.

use crate::DbPool;
use crate::error::DataError;
use crate::query::briefs::{JourneyBindingRow, select_bindings_by_journey_id};
use dapple_types::DateTime;
use dapple_types::model::journey_bindings::{BindingView, JourneyBindingsResponse};
use uuid::Uuid;

pub async fn build_bindings(
    db: &DbPool,
    journey_id: Uuid,
) -> Result<JourneyBindingsResponse, DataError> {
    let rows = select_bindings_by_journey_id(db, journey_id).await?;
    let bindings = rows.into_iter().map(binding_view_from_row).collect();

    Ok(JourneyBindingsResponse {
        journey_id,
        bindings,
    })
}

fn binding_view_from_row(row: JourneyBindingRow) -> BindingView {
    BindingView {
        value_slot_id: row.value_slot_id,
        value_kind: row.value_kind,
        text_value: row.text_value,
        int_value: row.int_value,
        number_value: row.number_value,
        bool_value: row.bool_value,
        json_value: row.json_value,
        asset_id: row.asset_id,
        item_values: row.item_values,
        bound_at: DateTime::from(row.bound_at),
    }
}
