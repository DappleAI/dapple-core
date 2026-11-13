//! `POST /api/guides/ingest` on the external surface.
//!
//! Accepts a multipart tarball, same wire format as the internal ingest
//! endpoint at `dapple-prompt-api`. The difference is the
//! `NamespaceOverride` the handler stamps onto the ingest call: every
//! `rev.object_paths` row produced by this materialization lands under
//! `(namespace_company_id = caller.company_id, scope = 'external')`
//! instead of the dapple defaults.
//!
//! The override is implemented via Postgres GUCs (`SET LOCAL`-style via
//! `set_config(..., is_local=true)`) inside the materialization
//! transaction. See
//! `crates/dapple-data/assets/sqlc/shared/insert_object_paths.sqlc` for the
//! reader side and
//! `crates/dapple-prompt-api/src/handler/guide_ingest.rs::ingest_guide_tarball`
//! for the writer side.
//!
//! V1 limitation: assets in customer guides land in the dapple namespace
//! because the asset-upload path runs outside the SET-LOCAL transaction.
//! Real customer-uploaded asset support requires threading the tx through
//! `upload_assets` and the underlying draft/chunk inserts — deferred until
//! a customer actually needs it.

use crate::{ExternalAppState, ExternalCaller};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::query::guides::guide_path_in_namespace;
use dapple_prompt_api::AppState;
use dapple_prompt_api::handler::guide_ingest::{
    IngestMode, IngestVersionOutcome, NamespaceOverride, extract_tarball_field,
    ingest_guide_tarball, ingest_guide_version_tarball,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// Query parameters accepted by the PATCH ingest handler. Mirrors the
/// internal surface's `IngestQueryParams` for the subset relevant to
/// external callers: `?backfill=true` flips `IngestMode::ForceRepair`,
/// which causes `refresh_existing_guide` to repair derived state
/// (including `rev.journey_brief_packs`) for journeys created before
/// the auto-derive code path landed. Defaults to `false`, so existing
/// SDK callers and the typical "re-sync this guide" workflow continue
/// to run the cheaper Normal three-way dedup.
#[derive(Debug, Deserialize, Default)]
pub struct ExternalPatchIngestQuery {
    #[serde(default)]
    pub backfill: bool,
}

/// `POST /api/guides/ingest` — customer-namespace tarball ingest.
pub async fn guide_ingest_post_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    multipart: Multipart,
) -> impl IntoApiResponse {
    let tarball_bytes = match extract_tarball_field(multipart).await {
        Ok(bytes) => bytes,
        Err(e) => return ingest_error(StatusCode::BAD_REQUEST, e),
    };

    // The external surface reuses the internal materialize_guide pipeline
    // via `ingest_guide_tarball`. We construct a transient `AppState` so
    // we can call the pipeline; conduit_url/flow_url stay None because
    // external ingest doesn't trigger embedding generation or dispatch
    // execution (those run after journey creation, not after ingest).
    let internal_state = Arc::new(AppState {
        db: Arc::new(dapple_data::DbPool::from_pool(state.db.get_pool().clone())),
        conduit_url: None,
        flow_url: None,
    });

    let override_ = NamespaceOverride {
        namespace_company_id: caller.company_id,
        scope: "external".to_string(),
    };

    match ingest_guide_tarball(&internal_state, &tarball_bytes, None, Some(override_)).await {
        Ok(response) => (
            StatusCode::CREATED,
            Json(json!({
                "status": "ok",
                "guide": response.guide,
                "derived_objects": response.derived_objects,
            })),
        )
            .into_response(),
        Err((status, msg)) => ingest_error(status, msg),
    }
}

fn ingest_error(status: StatusCode, message: String) -> axum::response::Response {
    log::error!("external ingest failed: status={}, msg={}", status, message);
    (
        status,
        Json(json!({
            "error": "INGEST_FAILED",
            "message": message,
        })),
    )
        .into_response()
}

/// `PATCH /api/guides/{path_id}/ingest` — re-ingest an existing
/// customer-namespace guide. The `path_id` is namespace-gated: a foreign or
/// unknown UUID returns the same 404 `RESOURCE_NOT_FOUND` body to avoid
/// existence leaks across namespaces. On a content-hash match the underlying
/// `refresh_existing_guide` branch returns 200 OK; on a new hash the
/// `materialize_guide_version_with_hash` branch returns 201 Created. Both
/// branches run under the customer's namespace GUC, so any
/// newly-materialized derived `object_paths` land at
/// `(namespace_company_id = caller.company_id, scope = 'external')` rather
/// than leaking into the dapple namespace.
pub async fn guide_ingest_patch_handler(
    State(state): State<Arc<ExternalAppState>>,
    caller: ExternalCaller,
    Path(path_id): Path<Uuid>,
    Query(query): Query<ExternalPatchIngestQuery>,
    multipart: Multipart,
) -> impl IntoApiResponse {
    // 1. Namespace-gate the target path. A 404 here is intentional even when
    //    the path exists in another customer's namespace — same body shape
    //    as the journeys handler so cross-namespace lookups can't be used to
    //    enumerate path_ids.
    match guide_path_in_namespace(&state.db, path_id, caller.company_id).await {
        Ok(true) => (),
        Ok(false) => return not_found("guide path", path_id),
        Err(e) => {
            log::error!("guide_path_in_namespace failed: {}", e);
            return ingest_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "namespace lookup failed".to_string(),
            );
        }
    }

    // 2. Extract the tarball.
    let tarball_bytes = match extract_tarball_field(multipart).await {
        Ok(bytes) => bytes,
        Err(e) => return ingest_error(StatusCode::BAD_REQUEST, e),
    };

    // 3. Build the same transient internal AppState the POST handler uses.
    let internal_state = Arc::new(AppState {
        db: Arc::new(dapple_data::DbPool::from_pool(state.db.get_pool().clone())),
        conduit_url: None,
        flow_url: None,
    });

    let override_ = NamespaceOverride {
        namespace_company_id: caller.company_id,
        scope: "external".to_string(),
    };

    // 4. Run the unified PATCH pipeline (refresh-or-new-version) under the
    //    customer's namespace GUC. `?backfill=true` flips to ForceRepair so
    //    the refresh branch also runs the journey-brief-packs repair (and
    //    any other ForceRepair-only repairs in derived materializers); the
    //    default Normal mode preserves the cheap three-way dedup for routine
    //    re-syncs.
    let ingest_mode = if query.backfill {
        IngestMode::ForceRepair
    } else {
        IngestMode::Normal
    };

    match ingest_guide_version_tarball(
        &internal_state,
        path_id,
        &tarball_bytes,
        None,
        Some(override_),
        ingest_mode,
    )
    .await
    {
        Ok(IngestVersionOutcome::Refreshed(refresh)) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "outcome": "refreshed",
                "guide_id": refresh.guide_id,
                "path_id": refresh.path_id,
                "chapters_processed": refresh.chapters_processed,
                "blocks_added": refresh.blocks_added,
                "chapters_repaired": refresh.chapters_repaired,
            })),
        )
            .into_response(),
        Ok(IngestVersionOutcome::NewVersion(response)) => (
            StatusCode::CREATED,
            Json(json!({
                "status": "ok",
                "outcome": "new_version",
                "guide": response.guide,
                "derived_objects": response.derived_objects,
            })),
        )
            .into_response(),
        Err((status, msg)) => ingest_error(status, msg),
    }
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
