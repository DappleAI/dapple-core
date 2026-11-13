//! External-surface mirrors of the Phase 2 chain endpoints:
//!
//! - `GET /api/journeys/chains?guide_id=<uuid>` — chain summaries for the
//!   caller's company, scoped to one guide.
//! - `GET /api/journeys/{id}/chain` — root→leaf chain for one journey,
//!   namespace-gated.
//!
//! Both scope by `caller.company_id`. Foreign / unknown ids return 404
//! with identical bodies (no existence leak).

use crate::ExternalAppState;
use crate::ExternalCaller;
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::error::DataError;
use dapple_data::model::journeys::{ChainJourneyRow, ChainSummaryRow, Journey};
use dapple_data::query::journeys::{
    select_chain_journeys_from_id, select_chain_summaries_by_principal_and_guide,
    select_journey_chain_ancestor,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

#[derive(JsonSchema, Debug, Deserialize, Serialize)]
pub struct ChainsIndexQuery {
    pub guide_id: Uuid,
}

pub async fn chains_index_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Query(query): Query<ChainsIndexQuery>,
) -> impl IntoApiResponse {
    let rows = match select_chain_summaries_by_principal_and_guide(
        &state.db,
        caller.principal_id,
        query.guide_id,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => return db_error("select_chain_summaries", e).into_response(),
    };
    let summaries = ChainSummaryRow::fold(rows);
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "results": summaries.len(),
            "objs": summaries,
        })),
    )
        .into_response()
}

pub async fn chain_get_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(id): Path<Uuid>,
) -> impl IntoApiResponse {
    // Namespace gate: requested journey must live in the caller's
    // `owner_company_id`. Foreign + unknown ids both return 404.
    let ancestor = match select_journey_chain_ancestor(&state.db, id).await {
        Ok(opt) => opt,
        Err(e) => return db_error("select_journey_chain_ancestor", e).into_response(),
    };
    match ancestor {
        Some(a) if a.owner_company_id == caller.company_id => (),
        _ => return not_found(id).into_response(),
    }

    let rows = match select_chain_journeys_from_id(&state.db, id, caller.principal_id).await {
        Ok(r) => r,
        Err(e) => return db_error("select_chain_journeys_from_id", e).into_response(),
    };
    let dtos: Vec<Journey> = rows.into_iter().map(chain_row_to_dto).collect();
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "results": dtos.len(),
            "objs": dtos,
        })),
    )
        .into_response()
}

fn chain_row_to_dto(row: ChainJourneyRow) -> Journey {
    Journey {
        id: row.id,
        name: row.name,
        principal_id: row.principal_id,
        user_id: None,
        playbook_id: row.playbook_id,
        brief_packs: Vec::new(),
        inference_profiles: Vec::new(),
        created_at: row.created_at.into(),
        hidden_at: row.hidden_at.map(Into::into),
        predecessor_journey_id: row.predecessor_journey_id,
        guide_id: row.guide_id,
        fork_stage_id: row.fork_stage_id,
    }
}

fn not_found(id: Uuid) -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "RESOURCE_NOT_FOUND",
            "message": format!("journey {id} not found in caller's namespace"),
        })),
    )
        .into_response()
}

fn db_error(op: &str, e: DataError) -> axum::response::Response {
    log::error!("{op}: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "DATABASE_ERROR",
            "message": format!("{op} failed"),
        })),
    )
        .into_response()
}
