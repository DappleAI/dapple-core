use crate::extract_path_uuid::ValidatedPathUuid;
use crate::extract_qs::QsQuery as Query;
use crate::handler::{get_response_handler, index_response_handler, post_response_handler};
use crate::schema::journeys::{CreateJourneySetSchema, GetJourneySetSchema, UpdateJourneySchema};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use dapple_data::model::journeys::{
    BriefPackRef, InferenceProfileRef, InsertJourneyArgs, InsertJourneyBriefPacksArgs,
    InsertJourneyInferenceProfilesArgs, Journey,
};
use dapple_data::query::guides::{guide_exposes_playbook, select_brief_pack_ids_for_guide};
use dapple_data::query::journeys::{
    insert_journey_brief_packs, insert_journey_inference_profiles, insert_journeys,
    playbook_stage_in_playbook, select_briefs_by_journey_object_id, select_journey_by_universal_id,
    select_journey_chain_ancestor, select_journeys_by_universal_id_by_playbooks,
    select_journeys_by_universal_ids, select_journeys_by_user,
    select_stage_binding_status_for_journey, select_step_binding_status_for_journey,
};
use dapple_data::query::principal_journey_preferences::insert_principal_journey_preference;
use dapple_data::query::principals::{
    resolve_billable_entity, select_human_principal_for_user_object_id,
};
use log::info;
use serde_json::Value;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

/// Resolve the viewer's `rev.principals.id` (their human_principal row) from
/// the authenticated user. Used by every journey-reading handler so SELECTs
/// can overlay the viewer's per-principal preferences. Returns the 500-ish
/// error tuple if the user has no human_principal row (means Mig-37 backfill
/// skipped them, which is recoverable but unusual).
async fn resolve_viewer_principal(
    db: &dapple_data::DbPool,
    user_object_id: Uuid,
) -> Result<Uuid, (StatusCode, Json<Value>)> {
    match select_human_principal_for_user_object_id(db, user_object_id).await {
        Ok(Some(pid)) => Ok(pid),
        Ok(None) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "NO_PRINCIPAL_FOR_USER",
                "message": format!("no human_principal for user_id={user_object_id}"),
            })),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "DATABASE_ERROR",
                "message": format!("resolve viewer principal: {e}"),
            })),
        )),
    }
}

pub async fn journeys_index_handler(
    opts: Query<GetJourneySetSchema>,
    State(data): State<Arc<AppState>>,
    user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    let Query(query) = opts;
    let viewer_principal_id = resolve_viewer_principal(&data.db, user.object_id()).await?;

    let objs = match query {
        GetJourneySetSchema::ByObjectIds { ids, options } => {
            select_journeys_by_universal_ids(
                &data.db,
                viewer_principal_id,
                ids,
                options.unwrap_or_default(),
            )
            .await
        }
        GetJourneySetSchema::ByPlaybook {
            playbook_id,
            options,
        } => {
            // select_journeys_by_universal_id_by_playbooks expects (pool, viewer_principal_id, universal_ids, playbook_ids, filter_ops).
            // For the single playbook index, treat `playbook_id` as the playbook_ids list and query all (no object ids),
            // so pass an empty universal_ids Vec.
            select_journeys_by_universal_id_by_playbooks(
                &data.db,
                viewer_principal_id,
                Vec::new(),
                vec![playbook_id],
                options.unwrap_or_default(),
            )
            .await
        }
        GetJourneySetSchema::ByUser { user_id, options } => {
            select_journeys_by_user(
                &data.db,
                viewer_principal_id,
                user_id,
                options.unwrap_or_default(),
            )
            .await
        }
    };

    Ok(index_response_handler("journeys", objs))
}

pub async fn journeys_post_handler(
    State(data): State<Arc<AppState>>,
    user: RegisteredUser,
    Json(body): Json<CreateJourneySetSchema>,
) -> impl IntoApiResponse {
    let user_id = user.object_id();

    // Resolve the caller's human_principal once for the whole batch (every
    // journey in a single POST attributes to the same actor).
    let principal_id = match select_human_principal_for_user_object_id(&data.db, user_id).await {
        Ok(Some(pid)) => pid,
        Ok(None) => {
            return post_response_handler::<Journey>(
                "journeys",
                Err(dapple_data::error::DataError::Custom(format!(
                    "no human_principal for user_id={user_id}; Mig 37 backfill skipped this user?",
                ))),
            );
        }
        Err(e) => return post_response_handler::<Journey>("journeys", Err(e)),
    };

    // Resolve billable_entity once for the whole batch (no subject_principal
    // in this flow — the user acts on their own behalf).
    let billable_entity_id = match resolve_billable_entity(&data.db, principal_id, None).await {
        Ok(opt) => opt,
        Err(e) => return post_response_handler::<Journey>("journeys", Err(e)),
    };

    // Step 0: Validate chain predecessors. Each create_schema with a
    // `predecessor_journey_id` must reference a journey the caller owns
    // (`principal_id == caller.principal_id`) and the request's `guide_id`
    // must equal the predecessor's `guide_id`. A `fork_stage_id` (stage-level
    // fork point, Migration 74) additionally requires a predecessor and must
    // be a stage of the predecessor's playbook. The DB triggers also enforce
    // these, but we pre-check for clean 4xx errors.
    for (idx, s) in body.objs.iter().enumerate() {
        if s.fork_stage_id.is_some() && s.predecessor_journey_id.is_none() {
            return post_response_handler::<Journey>(
                "journeys",
                Err(dapple_data::error::DataError::Custom(format!(
                    "journey[{idx}]: fork_stage_id requires predecessor_journey_id (a fork point names where to branch FROM)",
                ))),
            );
        }
        if let Some(pred_id) = s.predecessor_journey_id {
            let ancestor = match select_journey_chain_ancestor(&data.db, pred_id).await {
                Ok(opt) => opt,
                Err(e) => return post_response_handler::<Journey>("journeys", Err(e)),
            };
            let ancestor = match ancestor {
                Some(a) => a,
                None => {
                    return post_response_handler::<Journey>(
                        "journeys",
                        Err(dapple_data::error::DataError::Custom(format!(
                            "journey[{idx}]: predecessor_journey_id {pred_id} not found",
                        ))),
                    );
                }
            };
            if ancestor.principal_id != principal_id {
                return post_response_handler::<Journey>(
                    "journeys",
                    Err(dapple_data::error::DataError::Custom(format!(
                        "journey[{idx}]: predecessor_journey_id {pred_id} not owned by caller",
                    ))),
                );
            }
            match ancestor.guide_id {
                Some(pred_g) if pred_g == s.guide_id => (),
                Some(pred_g) => {
                    return post_response_handler::<Journey>(
                        "journeys",
                        Err(dapple_data::error::DataError::Custom(format!(
                            "journey[{idx}]: guide_id {} does not match predecessor's guide_id {pred_g}",
                            s.guide_id
                        ))),
                    );
                }
                None => {
                    return post_response_handler::<Journey>(
                        "journeys",
                        Err(dapple_data::error::DataError::Custom(format!(
                            "journey[{idx}]: predecessor {pred_id} has no guide_id; chain coherence requires matching guide pins",
                        ))),
                    );
                }
            }
            if let Some(fork_stage_id) = s.fork_stage_id {
                match playbook_stage_in_playbook(&data.db, ancestor.playbook_id, fork_stage_id)
                    .await
                {
                    Ok(true) => (),
                    Ok(false) => {
                        return post_response_handler::<Journey>(
                            "journeys",
                            Err(dapple_data::error::DataError::Custom(format!(
                                "journey[{idx}]: fork_stage_id {fork_stage_id} is not a stage of predecessor {pred_id}'s playbook",
                            ))),
                        );
                    }
                    Err(e) => return post_response_handler::<Journey>("journeys", Err(e)),
                }
            }
        }
    }

    // Step 0.5: Validate each (guide_id, playbook_id) pair is internally
    // consistent. The caller supplies BOTH as specific object_ids — it already
    // holds the exact guide it navigated to (browse is canonical-pin-aware) —
    // and the backend only checks the guide exposes that playbook version. We
    // do NOT resolve "latest"/by-path; canonical-pin selection is a browse
    // concern. A matching-but-non-canonical pair is a normal, valid request
    // (experiments / a consumer deliberately on an older version). The pin
    // lives in `journeys.guide_id` from create and never shifts underneath the
    // journey (re-ingest, resume, and chapter handoffs all keep it).
    let mut resolved_guide_ids: Vec<Uuid> = Vec::with_capacity(body.objs.len());
    for (idx, s) in body.objs.iter().enumerate() {
        match guide_exposes_playbook(&data.db, s.guide_id, s.playbook_id).await {
            Ok(true) => {}
            Ok(false) => {
                return post_response_handler::<Journey>(
                    "journeys",
                    Err(dapple_data::error::DataError::Custom(format!(
                        "journey[{idx}]: guide_id {} does not expose playbook_id {} (GUIDE_PLAYBOOK_MISMATCH); pass a (guide_id, playbook_id) pair from the same guide version.",
                        s.guide_id, s.playbook_id
                    ))),
                );
            }
            Err(e) => return post_response_handler::<Journey>("journeys", Err(e)),
        }
        resolved_guide_ids.push(s.guide_id);
    }

    // Step 1: Build InsertJourneyArgs
    let objs = {
        body.objs
            .iter()
            .enumerate()
            .fold(Vec::new(), |mut objs, (idx, s)| {
                objs.push(InsertJourneyArgs {
                    playbook_ids: vec![s.playbook_id],
                    principal_ids: vec![principal_id],
                    subject_principal_ids: vec![None],
                    billable_entity_ids: vec![billable_entity_id],
                    names: vec![s.name.clone()],
                    owner_company_ids: vec![dapple_data::DAPPLE_COMPANY_UUID],
                    predecessor_journey_ids: vec![s.predecessor_journey_id],
                    guide_ids: vec![Some(resolved_guide_ids[idx])],
                    fork_stage_ids: vec![s.fork_stage_id],
                });
                objs
            })
    };

    info!(
        "journeys_post_handler: creating {} journey objs",
        objs.len()
    );
    info!(
        "journeys_post_handler: column lengths: playbooks={}, principals={}, names={}",
        objs.iter().map(|o| o.playbook_ids.len()).sum::<usize>(),
        objs.iter().map(|o| o.principal_ids.len()).sum::<usize>(),
        objs.iter().map(|o| o.names.len()).sum::<usize>()
    );

    // Step 2: Insert the journeys (round-trip 1)
    let inserted_journeys = match insert_journeys(&data.db, objs).await {
        Ok(journeys) => journeys,
        Err(e) => return post_response_handler::<Journey>("journeys", Err(e)),
    };

    // Step 3: Build journey_brief_packs associations.
    //
    // Auto-derive policy: when a caller leaves `brief_pack_ids` empty on a
    // single create_schema, the server walks `journey.guide_id →
    // guide_brief_packs` to fill in the brief_packs the pinned guide
    // declares. Resolution flows through the journey's pinned guide_id
    // (Step 0.5) so re-ingest never shifts an existing journey's
    // attachments. Clients holding only a `playbook_id` (dapple-ui-demo,
    // dapple-suite, acme) get correct, version-stable defaults.
    //
    // Caller-supplied `brief_pack_ids` take precedence so explicit-pass
    // scenarios keep working. Per-schema policy means a single batch can
    // mix auto-derived and explicit journeys.
    let mut bp_journey_ids: Vec<Uuid> = Vec::new();
    let mut bp_pack_ids: Vec<Uuid> = Vec::new();
    let mut derived_for_response: Vec<Vec<Uuid>> = Vec::with_capacity(body.objs.len());

    for (idx, create_schema) in body.objs.iter().enumerate() {
        let Some(journey) = inserted_journeys.get(idx) else {
            derived_for_response.push(Vec::new());
            continue;
        };
        let ids_for_this_journey: Vec<Uuid> = if create_schema.brief_pack_ids.is_empty() {
            match select_brief_pack_ids_for_guide(&data.db, resolved_guide_ids[idx]).await {
                Ok(ids) => ids,
                Err(e) => return post_response_handler::<Journey>("journeys", Err(e)),
            }
        } else {
            create_schema.brief_pack_ids.clone()
        };
        for bp_id in &ids_for_this_journey {
            bp_journey_ids.push(journey.id);
            bp_pack_ids.push(*bp_id);
        }
        derived_for_response.push(ids_for_this_journey);
    }

    // Step 4: Insert brief_pack associations if any (round-trip 2)
    if !bp_journey_ids.is_empty() {
        let assoc_args = InsertJourneyBriefPacksArgs {
            journey_ids: bp_journey_ids,
            brief_pack_ids: bp_pack_ids,
        };

        if let Err(e) = insert_journey_brief_packs(&data.db, assoc_args).await {
            return post_response_handler::<Journey>("journeys", Err(e));
        }
    }

    // Step 5: Build journey_inference_profiles associations
    let mut ip_journey_ids: Vec<Uuid> = Vec::new();
    let mut inference_profile_ids: Vec<Uuid> = Vec::new();

    for (idx, create_schema) in body.objs.iter().enumerate() {
        if let Some(journey) = inserted_journeys.get(idx) {
            for ip_id in create_schema.inference_profile_ids.iter() {
                ip_journey_ids.push(journey.id);
                inference_profile_ids.push(*ip_id);
            }
        }
    }

    // Step 6: Insert inference_profile associations if any (round-trip 3)
    if !ip_journey_ids.is_empty() {
        let assoc_args = InsertJourneyInferenceProfilesArgs {
            journey_ids: ip_journey_ids,
            inference_profile_ids,
        };

        if let Err(e) = insert_journey_inference_profiles(&data.db, assoc_args).await {
            return post_response_handler::<Journey>("journeys", Err(e));
        }
    }

    // Step 7: Convert to Journey DTOs. brief_packs uses the resolved set
    // (caller-supplied or auto-derived) so the response shows what was
    // actually associated; inference_profiles pass through from the request
    // because there's no auto-derive equivalent for them in V1.
    let journeys: Vec<Journey> = inserted_journeys
        .into_iter()
        .zip(body.objs.iter())
        .enumerate()
        .map(|(idx, (mut j, schema))| {
            let resolved_bp_ids = derived_for_response
                .get(idx)
                .cloned()
                .unwrap_or_else(|| schema.brief_pack_ids.clone());
            j.brief_packs = resolved_bp_ids
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

    post_response_handler("journeys", Ok(journeys))
}

pub async fn journeys_get_handler(
    ValidatedPathUuid(id): ValidatedPathUuid,
    State(data): State<Arc<AppState>>,
    user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    let viewer_principal_id = resolve_viewer_principal(&data.db, user.object_id()).await?;
    get_response_handler(
        "journey",
        select_journey_by_universal_id(&data.db, viewer_principal_id, id).await,
    )
}

/// PATCH /api/journeys/{id} — rename the journey from the *viewer's*
/// perspective. Writes a fresh `rev.principal_journey_preferences` row with
/// `name = Some(...)` and `hidden_at = NULL`; the journey row itself is
/// untouched. Other principals still see their own (possibly different) name.
pub async fn journeys_patch_handler(
    ValidatedPathUuid(id): ValidatedPathUuid,
    State(data): State<Arc<AppState>>,
    user: RegisteredUser,
    Json(body): Json<UpdateJourneySchema>,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    let viewer_principal_id = resolve_viewer_principal(&data.db, user.object_id()).await?;

    // Confirm the journey exists before touching prefs. The single-get does
    // not filter on hidden_at, so a freshly hidden journey can still be
    // renamed (which un-hides it: the new prefs row has hidden_at = NULL).
    if let Err(e) = select_journey_by_universal_id(&data.db, viewer_principal_id, id).await {
        return get_response_handler::<Journey>("journey", Err(e));
    }

    if let Err(e) = insert_principal_journey_preference(
        &data.db,
        viewer_principal_id,
        id,
        Some(body.name),
        None,
    )
    .await
    {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "DATABASE_ERROR",
                "message": format!("insert_principal_journey_preference: {e}"),
            })),
        ));
    }

    get_response_handler(
        "journey",
        select_journey_by_universal_id(&data.db, viewer_principal_id, id).await,
    )
}

/// DELETE /api/journeys/{id} — soft-hide from the viewer's lists. Writes a
/// fresh `rev.principal_journey_preferences` row with `name = NULL` and
/// `hidden_at = now()`. Idempotent: subsequent DELETEs append more rows but
/// the resolved hidden_at remains set; the journey itself and other
/// principals' views are unaffected.
pub async fn journeys_delete_handler(
    ValidatedPathUuid(id): ValidatedPathUuid,
    State(data): State<Arc<AppState>>,
    user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    let viewer_principal_id = resolve_viewer_principal(&data.db, user.object_id()).await?;

    if let Err(e) = select_journey_by_universal_id(&data.db, viewer_principal_id, id).await {
        return get_response_handler::<Journey>("journey", Err(e));
    }

    let now = OffsetDateTime::now_utc();
    if let Err(e) =
        insert_principal_journey_preference(&data.db, viewer_principal_id, id, None, Some(now))
            .await
    {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "DATABASE_ERROR",
                "message": format!("insert_principal_journey_preference: {e}"),
            })),
        ));
    }

    get_response_handler(
        "journey",
        select_journey_by_universal_id(&data.db, viewer_principal_id, id).await,
    )
}

pub async fn journeys_briefs_index_handler(
    ValidatedPathUuid(journey_id): ValidatedPathUuid,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    index_response_handler(
        "journey_briefs",
        select_briefs_by_journey_object_id(&data.db, journey_id).await,
    )
}

/// Best-effort re-drive of a journey's auto-dispatch cascade on each poll, so a
/// stalled journey self-heals (recovers a lost in-flight retrigger — e.g. a pod
/// restart during a long task). Spawned so it never blocks the poll.
///
/// The drain must run where execution runs. When `flow_url` is `None` execution
/// is in-process here, so we reconcile in-process. When a flow service URL is
/// configured, execution is delegated to the flow pod — so we delegate the
/// reconcile too, via `POST {flow_url}/reconcile`, instead of running the drain
/// in the wrong process (the historic `flow_url.is_none()` gate left flow mode
/// with no reconcile at all, permanently stalling multi-stage journeys there).
fn spawn_reconcile(data: &Arc<AppState>, journey_id: Uuid) {
    match data.flow_url.as_deref() {
        None => {
            tokio::spawn(
                dapple_prompt_flow::dispatch_runner::reconcile_journey_dispatches(
                    (*data.db).clone(),
                    journey_id,
                    4,
                ),
            );
        }
        Some(flow_url) => {
            let url = format!("{}/reconcile", flow_url);
            let signing_key = std::env::var("DAPPLE_PROMPT_API_SIGNING_KEY").unwrap_or_default();
            let payload = serde_json::json!({ "journey_id": journey_id });
            tokio::spawn(async move {
                let client = reqwest::Client::new();
                if let Err(e) = client
                    .post(&url)
                    .bearer_auth(&signing_key)
                    .json(&payload)
                    .send()
                    .await
                {
                    log::debug!(
                        "reconcile delegate to flow failed for journey {}: {}",
                        journey_id,
                        e
                    );
                }
            });
        }
    }
}

/// Per-stage completion for a journey: one entry per playbook stage with
/// `done` = true once every value slot the stage exposes (its overlays) is
/// bound somewhere in the journey chain. A stage with no overlays is trivially
/// done (strict default). Drives dapple-ui's per-stage/chapter progress.
pub async fn journeys_stages_index_handler(
    ValidatedPathUuid(journey_id): ValidatedPathUuid,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    // Reconcile-on-poll: re-drive a stalled cascade (in-process or delegated to the
    // flow pod depending on `flow_url`). See `spawn_reconcile`.
    spawn_reconcile(&data, journey_id);
    index_response_handler(
        "journey_stages",
        select_stage_binding_status_for_journey(&data.db, journey_id).await,
    )
}

/// Per-step exposed-slot status for a journey: one entry per playbook step with
/// `done` = true once every value slot the step exposes is bound in the journey
/// chain, plus `exposed_slots` carrying each slot's bind target (value_slot_id),
/// kind, and bound flag. Finer-grained than `/stages`; lets a client discover
/// which step (e.g. a generate→pick→refine pick step) is waiting on which slot.
pub async fn journeys_steps_index_handler(
    ValidatedPathUuid(journey_id): ValidatedPathUuid,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    // Reconcile-on-poll: see `spawn_reconcile`. The UI polls both /stages and
    // /steps, so either endpoint re-drives a stalled journey.
    spawn_reconcile(&data, journey_id);
    index_response_handler(
        "journey_steps",
        select_step_binding_status_for_journey(&data.db, journey_id).await,
    )
}

pub async fn journeys_recent_handler(
    State(data): State<Arc<AppState>>,
    user: RegisteredUser,
) -> Result<impl IntoApiResponse, (StatusCode, Json<Value>)> {
    let viewer_principal_id = resolve_viewer_principal(&data.db, user.object_id()).await?;
    let filter_ops = dapple_data::FilterOptions {
        limit: Some(10),
        page: Some(1),
    };

    Ok(index_response_handler(
        "journeys",
        select_journeys_by_user(&data.db, viewer_principal_id, user.object_id(), filter_ops).await,
    ))
}
