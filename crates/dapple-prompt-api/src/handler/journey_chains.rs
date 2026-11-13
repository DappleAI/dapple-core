//! Phase 2 chains endpoints (Migration 44).
//!
//! - `GET /api/journeys/chains?guide_id=<uuid>` — list all chain summaries
//!   the caller owns for one guide. One entry per chain root, with the
//!   list of leaves (branches) under it. UI renders one tile per
//!   `ChainSummary`; multi-branch summaries get a "forked" indicator.
//! - `GET /api/journeys/{id}/chain` — the chain (root → leaf) reaching
//!   the requested journey. Used by the chapter resolver to drop the
//!   "latest journey per playbook" lookup that breaks under concurrent
//!   essays.
//!
//! Both gated by `RegisteredUser`; both scope by the caller's
//! `principal_id`. Foreign or unknown ids return 404 with the canonical
//! body — no cross-user existence leak.

use crate::AppState;
use crate::RegisteredUser;
use crate::extract_path_uuid::ValidatedPathUuid;
use crate::extract_qs::QsQuery as Query;
use crate::handler::index_response_handler;
use crate::schema::journeys::ChainsIndexQuery;
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::model::journeys::{ChainJourneyRow, ChainSummaryRow, Journey};
use dapple_data::query::journeys::{
    select_chain_journeys_from_id, select_chain_summaries_by_principal_and_guide,
    select_journey_chain_ancestor,
};
use dapple_data::query::principals::select_human_principal_for_user_object_id;
// `Journey` is re-exported from `dapple_types::model::journeys` via
// `dapple_data::model::journeys`'s `pub use` at the top of that module.
// The `created_at: DateTime` field is also re-exported through the same
// glob; reach it via the public DTO path.
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

pub async fn chains_index_handler(
    Query(query): Query<ChainsIndexQuery>,
    State(data): State<Arc<AppState>>,
    user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    let principal_id = resolve_caller_principal(&data.db, user.object_id()).await?;
    let summaries =
        select_chain_summaries_by_principal_and_guide(&data.db, principal_id, query.guide_id)
            .await
            .map(ChainSummaryRow::fold);
    Ok(index_response_handler("chains", summaries))
}

pub async fn chain_get_handler(
    ValidatedPathUuid(id): ValidatedPathUuid,
    State(data): State<Arc<AppState>>,
    user: RegisteredUser,
) -> impl IntoApiResponse {
    let principal_id = match resolve_caller_principal(&data.db, user.object_id()).await {
        Ok(p) => p,
        Err((status, body)) => return (status, body).into_response(),
    };

    // Ownership gate: the requested journey must belong to the caller's
    // principal. Foreign + unknown ids both return 404 with the same body
    // to avoid existence leaks.
    let ancestor = match select_journey_chain_ancestor(&data.db, id).await {
        Ok(opt) => opt,
        Err(e) => return db_error("select_journey_chain_ancestor", e).into_response(),
    };
    match ancestor {
        Some(a) if a.principal_id == principal_id => (),
        _ => return not_found_chain(id).into_response(),
    }

    let rows = match select_chain_journeys_from_id(&data.db, id, principal_id).await {
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
        user_id: None, // chain endpoint doesn't surface the legacy alias
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

fn not_found_chain(id: Uuid) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "RESOURCE_NOT_FOUND",
            "message": format!("journey {id} not found"),
        })),
    )
}

fn db_error(op: &str, e: dapple_data::error::DataError) -> (StatusCode, Json<Value>) {
    log::error!("{op}: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "DATABASE_ERROR",
            "message": format!("{op} failed"),
        })),
    )
}

async fn resolve_caller_principal(
    db: &dapple_data::DbPool,
    user_object_id: Uuid,
) -> Result<Uuid, (StatusCode, Json<Value>)> {
    match select_human_principal_for_user_object_id(db, user_object_id).await {
        Ok(Some(pid)) => Ok(pid),
        Ok(None) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "NO_PRINCIPAL_FOR_USER",
                "message": format!("no human_principal for user_id={user_object_id}"),
            })),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "DATABASE_ERROR",
                "message": format!("resolve viewer principal: {e}"),
            })),
        )),
    }
}
