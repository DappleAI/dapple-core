//! `POST /api/journeys` on the external surface.
//!
//! Mirrors `dapple-prompt-api::handler::journeys::journeys_post_handler` but
//! every UUID the customer references — `playbook_id`, `brief_pack_ids`,
//! `inference_profile_ids` — is namespace-checked before the insert. The
//! created journey carries `owner_company_id = caller.company_id` and
//! `user_id = caller.service_user_object_id` (resolved through user_details).

use crate::schema::journeys::{
    CreateExternalJourneySchema, CreateExternalJourneySetSchema, ListExternalJourneysQuery,
    UpdateExternalJourneySchema,
};
use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::model::journeys::{BriefPackRef, InferenceProfileRef, Journey};
use dapple_data::model::journeys::{
    InsertJourneyArgs, InsertJourneyBriefPacksArgs, InsertJourneyInferenceProfilesArgs,
};
use dapple_data::query::brief_packs::brief_packs_in_namespace;
use dapple_data::query::guides::{
    guide_exposes_playbook, guide_in_namespace, select_brief_pack_ids_for_guide,
};
use dapple_data::query::inference_profiles::inference_profiles_in_namespace;
use dapple_data::query::journeys::{
    insert_journey_brief_packs, insert_journey_inference_profiles, insert_journeys,
    journey_in_namespace, playbook_stage_in_playbook, select_journey_by_universal_id,
    select_journey_chain_ancestor, select_journeys_by_owner_company_id,
};
use dapple_data::query::playbooks::playbook_in_namespace;
use dapple_data::query::principal_journey_preferences::insert_principal_journey_preference;
use dapple_data::query::principals::resolve_billable_entity;
use serde_json::json;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

pub async fn journeys_post_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Json(body): Json<CreateExternalJourneySetSchema>,
) -> impl IntoApiResponse {
    if body.objs.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "EMPTY_BATCH", "message": "objs must be non-empty" })),
        )
            .into_response();
    }

    // Validate every referenced UUID belongs to the caller's namespace.
    for obj in body.objs.iter() {
        match validate_obj_namespace(&state, &caller, obj).await {
            Ok(()) => (),
            Err(resp) => return resp,
        }
    }

    // Validate chain predecessors: each predecessor must live in the
    // caller's `owner_company_id` namespace and have a `guide_id` matching
    // the request's `guide_id`. Foreign predecessors return 404 with the
    // same body as unknown ones (no existence leak). A `fork_stage_id`
    // (stage-level fork point, Migration 74) additionally requires a
    // predecessor and must be a stage of the predecessor's playbook.
    for (idx, obj) in body.objs.iter().enumerate() {
        if obj.fork_stage_id.is_some() && obj.predecessor_journey_id.is_none() {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "FORK_REQUIRES_PREDECESSOR",
                    "message": format!(
                        "journey[{idx}]: fork_stage_id requires predecessor_journey_id",
                    ),
                })),
            )
                .into_response();
        }
        if let Some(pred_id) = obj.predecessor_journey_id {
            let ancestor = match select_journey_chain_ancestor(&state.db, pred_id).await {
                Ok(opt) => opt,
                Err(e) => return db_error("select_journey_chain_ancestor", e),
            };
            let ancestor = match ancestor {
                Some(a) if a.owner_company_id == caller.company_id => a,
                _ => return not_found("predecessor_journey", pred_id),
            };
            match ancestor.guide_id {
                Some(pred_g) if pred_g == obj.guide_id => (),
                _ => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": "JOURNEY_CHAIN_GUIDE_MISMATCH",
                            "message": format!(
                                "journey[{idx}] guide_id must match predecessor's guide_id",
                            ),
                        })),
                    )
                        .into_response();
                }
            }
            if let Some(fork_stage_id) = obj.fork_stage_id {
                match playbook_stage_in_playbook(&state.db, ancestor.playbook_id, fork_stage_id)
                    .await
                {
                    Ok(true) => (),
                    Ok(false) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "error": "FORK_STAGE_NOT_IN_PREDECESSOR_PLAYBOOK",
                                "message": format!(
                                    "journey[{idx}]: fork_stage_id {fork_stage_id} is not a stage of the predecessor's playbook",
                                ),
                            })),
                        )
                            .into_response();
                    }
                    Err(e) => return db_error("playbook_stage_in_playbook", e),
                }
            }
        }
    }

    // Resolve billable_entity once for the whole batch — every journey in
    // a single POST attributes to the same paying principal.
    let billable_entity_id =
        match resolve_billable_entity(&state.db, caller.principal_id, None).await {
            Ok(opt) => opt,
            Err(e) => return db_error("resolve_billable_entity", e),
        };

    // Validate each (guide_id, playbook_id) pair is internally consistent. The
    // caller supplies BOTH as specific object_ids (it holds the exact guide it
    // navigated to, from the canonical-pin-aware browse); the backend only
    // checks the guide exposes that playbook version. No "latest"/by-path
    // resolution — canonical-pin selection is a browse concern. A matching-but-
    // non-canonical pair is normal and valid. Customer journeys are especially
    // sensitive to re-ingest drift, so the pin is the caller's exact guide_id
    // and never shifts. (guide_id namespace was checked in validate_obj_namespace.)
    let mut resolved_guide_ids: Vec<Uuid> = Vec::with_capacity(body.objs.len());
    for (idx, s) in body.objs.iter().enumerate() {
        match guide_exposes_playbook(&state.db, s.guide_id, s.playbook_id).await {
            Ok(true) => {}
            Ok(false) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "GUIDE_PLAYBOOK_MISMATCH",
                        "message": format!(
                            "journey[{idx}]: guide_id {} does not expose playbook_id {}; pass a (guide_id, playbook_id) pair from the same guide version",
                            s.guide_id, s.playbook_id
                        ),
                    })),
                )
                    .into_response();
            }
            Err(e) => return db_error("guide_exposes_playbook", e),
        }
        resolved_guide_ids.push(s.guide_id);
    }

    // Build InsertJourneyArgs (one per obj). principal_id is the caller's
    // external_service_principal, stamped directly onto each journey.
    // subject_principal_id stays None in V1 (Shape D — acme acting on
    // behalf of one of their subjects — isn't wired through the handler yet).
    let objs: Vec<InsertJourneyArgs> = body
        .objs
        .iter()
        .enumerate()
        .map(|(idx, s)| InsertJourneyArgs {
            playbook_ids: vec![s.playbook_id],
            principal_ids: vec![caller.principal_id],
            subject_principal_ids: vec![None],
            billable_entity_ids: vec![billable_entity_id],
            names: vec![s.name.clone()],
            owner_company_ids: vec![caller.company_id],
            predecessor_journey_ids: vec![s.predecessor_journey_id],
            guide_ids: vec![Some(resolved_guide_ids[idx])],
            fork_stage_ids: vec![s.fork_stage_id],
        })
        .collect();

    let inserted = match insert_journeys(&state.db, objs).await {
        Ok(j) => j,
        Err(e) => return db_error("insert_journeys", e),
    };

    // Brief-pack associations (round-trip 2). Auto-derive policy matches
    // the internal surface: when the caller leaves brief_pack_ids empty
    // for a given create_schema, the server walks the source guide's
    // `guide_brief_packs` and attaches whatever brief_packs the guide
    // declares. Customer clients (Acme et al) typically only know
    // playbook_id; without this they'd ship journeys with zero
    // journey_brief_packs rows and break the brief-form downstream.
    //
    // Caller-supplied IDs were already namespace-validated by
    // validate_obj_namespace; auto-derived IDs are namespace-safe by
    // construction (they came from a `guide_playbooks` row whose
    // `playbook_id` was already validated).
    let mut bp_journey_ids: Vec<Uuid> = Vec::new();
    let mut bp_ids: Vec<Uuid> = Vec::new();
    let mut resolved_bp_per_schema: Vec<Vec<Uuid>> = Vec::with_capacity(body.objs.len());
    for (idx, schema) in body.objs.iter().enumerate() {
        let Some(journey) = inserted.get(idx) else {
            resolved_bp_per_schema.push(Vec::new());
            continue;
        };
        let ids: Vec<Uuid> = if schema.brief_pack_ids.is_empty() {
            match select_brief_pack_ids_for_guide(&state.db, resolved_guide_ids[idx]).await {
                Ok(ids) => ids,
                Err(e) => return db_error("select_brief_pack_ids_for_guide", e),
            }
        } else {
            schema.brief_pack_ids.clone()
        };
        for id in &ids {
            bp_journey_ids.push(journey.id);
            bp_ids.push(*id);
        }
        resolved_bp_per_schema.push(ids);
    }
    if !bp_journey_ids.is_empty()
        && let Err(e) = insert_journey_brief_packs(
            &state.db,
            InsertJourneyBriefPacksArgs {
                journey_ids: bp_journey_ids,
                brief_pack_ids: bp_ids,
            },
        )
        .await
    {
        return db_error("insert_journey_brief_packs", e);
    }

    // Inference-profile associations (round-trip 3)
    let (ip_journey_ids, ip_ids) =
        collect_associations(&inserted, &body.objs, |s| s.inference_profile_ids.clone());
    if !ip_journey_ids.is_empty()
        && let Err(e) = insert_journey_inference_profiles(
            &state.db,
            InsertJourneyInferenceProfilesArgs {
                journey_ids: ip_journey_ids,
                inference_profile_ids: ip_ids,
            },
        )
        .await
    {
        return db_error("insert_journey_inference_profiles", e);
    }

    // Convert to public DTOs. brief_packs uses the resolved set (auto-
    // derived OR caller-supplied) so the response reflects what is
    // actually associated; inference_profiles pass through from the
    // request because there's no auto-derive equivalent in V1.
    let journeys: Vec<Journey> = inserted
        .into_iter()
        .zip(body.objs.iter())
        .enumerate()
        .map(|(idx, (mut j, schema))| {
            let resolved_ids = resolved_bp_per_schema
                .get(idx)
                .cloned()
                .unwrap_or_else(|| schema.brief_pack_ids.clone());
            j.brief_packs = resolved_ids
                .into_iter()
                .map(|id| BriefPackRef { id })
                .collect();
            j.inference_profiles = schema
                .inference_profile_ids
                .iter()
                .map(|&id| InferenceProfileRef { id })
                .collect();
            j
        })
        .collect();

    (
        StatusCode::CREATED,
        Json(json!({
            "status": "ok",
            "results": journeys.len(),
            "objs": journeys,
        })),
    )
        .into_response()
}

/// Validate that every UUID referenced in this create-schema belongs to the
/// caller's namespace. Returns 404 on the first mismatch.
async fn validate_obj_namespace(
    state: &Arc<ExternalAppState>,
    caller: &ExternalCaller,
    obj: &CreateExternalJourneySchema,
) -> Result<(), axum::response::Response> {
    match playbook_in_namespace(&state.db, obj.playbook_id, caller.company_id).await {
        Ok(true) => (),
        Ok(false) => return Err(not_found("playbook", obj.playbook_id)),
        Err(e) => return Err(db_error("playbook_in_namespace", e)),
    }

    // The journey's pinned guide must also live in the caller's namespace. A
    // foreign/unknown guide_id returns the same 404 body (no existence leak);
    // the (guide_id, playbook_id) consistency check below is a separate gate.
    match guide_in_namespace(&state.db, obj.guide_id, caller.company_id).await {
        Ok(true) => (),
        Ok(false) => return Err(not_found("guide", obj.guide_id)),
        Err(e) => return Err(db_error("guide_in_namespace", e)),
    }

    if !obj.brief_pack_ids.is_empty() {
        match brief_packs_in_namespace(&state.db, &obj.brief_pack_ids, caller.company_id).await {
            Ok(true) => (),
            Ok(false) => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "BRIEF_PACK_NOT_IN_NAMESPACE",
                        "message": "one or more brief_pack_ids not found in caller's namespace",
                    })),
                )
                    .into_response());
            }
            Err(e) => return Err(db_error("brief_packs_in_namespace", e)),
        }
    }

    if !obj.inference_profile_ids.is_empty() {
        match inference_profiles_in_namespace(
            &state.db,
            &obj.inference_profile_ids,
            caller.company_id,
        )
        .await
        {
            Ok(true) => (),
            Ok(false) => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "INFERENCE_PROFILE_NOT_IN_NAMESPACE",
                        "message": "one or more inference_profile_ids not found in caller's namespace",
                    })),
                )
                    .into_response());
            }
            Err(e) => return Err(db_error("inference_profiles_in_namespace", e)),
        }
    }

    Ok(())
}

fn collect_associations<F>(
    inserted: &[Journey],
    schemas: &[CreateExternalJourneySchema],
    pluck: F,
) -> (Vec<Uuid>, Vec<Uuid>)
where
    F: Fn(&CreateExternalJourneySchema) -> Vec<Uuid>,
{
    let mut journey_ids = Vec::new();
    let mut assoc_ids = Vec::new();
    for (idx, schema) in schemas.iter().enumerate() {
        if let Some(journey) = inserted.get(idx) {
            for id in pluck(schema) {
                journey_ids.push(journey.id);
                assoc_ids.push(id);
            }
        }
    }
    (journey_ids, assoc_ids)
}

fn not_found(kind: &str, id: Uuid) -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "RESOURCE_NOT_FOUND",
            "message": format!("{} {} not found in caller's namespace", kind, id),
        })),
    )
        .into_response()
}

fn db_error(op: &str, e: dapple_data::error::DataError) -> axum::response::Response {
    log::error!("{}: {}", op, e);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "DATABASE_ERROR",
            "message": format!("{} failed", op),
        })),
    )
        .into_response()
}

/// PATCH /api/journeys/{id} — rename the journey from the caller's
/// perspective. Same shape as the internal handler: appends a fresh
/// `rev.principal_journey_preferences` row tied to `caller.principal_id`.
/// Gated by `journey_in_namespace` so customers cannot rename a journey
/// they don't own.
pub async fn journeys_patch_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateExternalJourneySchema>,
) -> impl IntoApiResponse {
    if !match journey_in_namespace(&state.db, id, caller.company_id).await {
        Ok(b) => b,
        Err(e) => return db_error("journey_in_namespace", e),
    } {
        return not_found("journey", id);
    }

    if let Err(e) = insert_principal_journey_preference(
        &state.db,
        caller.principal_id,
        id,
        Some(body.name),
        None,
    )
    .await
    {
        return db_error("insert_principal_journey_preference", e);
    }

    match select_journey_by_universal_id(&state.db, caller.principal_id, id).await {
        Ok(journey) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": 1,
                "obj": journey,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_journey_by_universal_id", e),
    }
}

/// DELETE /api/journeys/{id} — soft-hide the journey from the caller's lists
/// by appending a preferences row with `hidden_at = now()`. The journey
/// itself, its dispatches, and any other principal's view are untouched.
pub async fn journeys_delete_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(id): Path<Uuid>,
) -> impl IntoApiResponse {
    if !match journey_in_namespace(&state.db, id, caller.company_id).await {
        Ok(b) => b,
        Err(e) => return db_error("journey_in_namespace", e),
    } {
        return not_found("journey", id);
    }

    let now = OffsetDateTime::now_utc();
    if let Err(e) =
        insert_principal_journey_preference(&state.db, caller.principal_id, id, None, Some(now))
            .await
    {
        return db_error("insert_principal_journey_preference", e);
    }

    match select_journey_by_universal_id(&state.db, caller.principal_id, id).await {
        Ok(journey) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": 1,
                "obj": journey,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_journey_by_universal_id", e),
    }
}

/// GET /api/journeys/{id} — fetch a single journey in the caller's
/// namespace. Returns the same Journey DTO the PATCH/DELETE handlers
/// already return (overlaid with the caller's per-principal name/hidden
/// preferences). Namespace-gated via `journey_in_namespace`; a foreign
/// or unknown UUID both return the same 404 RESOURCE_NOT_FOUND body
/// (no cross-namespace existence leak).
pub async fn journeys_get_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(id): Path<Uuid>,
) -> impl IntoApiResponse {
    if !match journey_in_namespace(&state.db, id, caller.company_id).await {
        Ok(b) => b,
        Err(e) => return db_error("journey_in_namespace", e),
    } {
        return not_found("journey", id);
    }

    match select_journey_by_universal_id(&state.db, caller.principal_id, id).await {
        Ok(journey) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": 1,
                "obj": journey,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_journey_by_universal_id", e),
    }
}

/// GET /api/journeys — list journeys in the caller's namespace.
/// Filters by `owner_company_id = caller.company_id` at the SQL site;
/// the caller's principal_id supplies the viewer-preferences overlay
/// (so journeys the caller has soft-hidden are filtered out).
///
/// Pagination via flat `?page=&limit=` query params — matches the other
/// external list endpoints (`/api/guides`, `/api/playbooks`).
pub async fn journeys_index_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Query(query): Query<ListExternalJourneysQuery>,
) -> impl IntoApiResponse {
    let filter_ops = dapple_data::FilterOptions {
        page: query.page.map(|p| p as usize),
        limit: query.limit.map(|l| l as usize),
    };
    match select_journeys_by_owner_company_id(
        &state.db,
        caller.principal_id,
        caller.company_id,
        filter_ops,
    )
    .await
    {
        Ok(journeys) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "results": journeys.len(),
                "objs": journeys,
            })),
        )
            .into_response(),
        Err(e) => db_error("select_journeys_by_owner_company_id", e),
    }
}
