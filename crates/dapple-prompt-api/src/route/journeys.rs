use crate::AppState;
use crate::handler::journey_asset_uploads::{
    journey_asset_upload_delete_handler, journey_asset_uploads_index_handler,
    journey_asset_uploads_post_handler,
};
use crate::handler::journey_bindings::{journey_binding_history_handler, journey_bindings_handler};
use crate::handler::journey_brief_form::journey_brief_form_handler;
use crate::handler::journey_chains::{chain_get_handler, chains_index_handler};
use crate::handler::journeys::{
    journeys_briefs_index_handler, journeys_delete_handler, journeys_get_handler,
    journeys_index_handler, journeys_patch_handler, journeys_post_handler, journeys_recent_handler,
    journeys_stages_index_handler, journeys_steps_index_handler,
};
use crate::handler::user_favorite_journey_chains::{
    add_favorite_journey_chain_handler, check_favorite_journey_chain_handler,
    list_favorite_journey_chains_handler, remove_favorite_journey_chain_handler,
};
use crate::handler::{
    DeleteErrorResponse, GetErrorResponse, GetResponse, IndexErrorResponse, IndexResponse,
    PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::journeys::{ChainsIndexQuery, GetJourneySetSchema};
use crate::schema::user_favorite_journey_chains::{
    AddFavoriteJourneyChainResponse, CheckFavoriteJourneyChainResponse,
    ListFavoriteJourneyChainsResponse, RemoveFavoriteJourneyChainResponse,
};
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, patch_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::dispatch_intent_bindings::DispatchIntentBinding;
use dapple_data::model::journey_asset_uploads::JourneyAssetUpload;
use dapple_data::model::journey_bindings::JourneyBindingsResponse;
use dapple_data::model::journey_brief_form::JourneyBriefFormResponse;
use dapple_data::model::journeys::{
    ChainSummary, Journey, JourneyBrief, JourneyStageStatus, JourneyStepStatus,
};
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(journeys_post_handler, |op| {
                op.description("Create a new journey")
                    .tag("Journeys")
                    .response::<201, Json<PostResponse<Journey>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(journeys_index_handler, index_docs))
        .api_route(
            "/recent",
            get_with(journeys_recent_handler, |op| {
                op.description("Get the 10 most recent journeys for the current user")
                    .tag("Journeys")
                    .response::<200, Json<IndexResponse<Journey>>>()
                    .response::<422, Json<IndexErrorResponse>>()
                    .response::<500, Json<IndexErrorResponse>>()
            }),
        )
        .api_route(
            "/favorites",
            get_with(list_favorite_journey_chains_handler, |op| {
                op.description(
                    "List all favorite journey chains for the authenticated user. Favorites \
                     are anchored to the chain root (the user's 'essay'), so advance and \
                     fork transitions don't invalidate them.",
                )
                .tag("Journeys")
                .tag("Favorites")
                .response::<200, Json<ListFavoriteJourneyChainsResponse>>()
                .response::<500, Json<IndexErrorResponse>>()
            }),
        )
        .api_route("/chains", get_with(chains_index_handler, chains_index_docs))
        .api_route(
            "/{id}",
            get_with(journeys_get_handler, |op| {
                op.description(
                    "Retrieve a specific journey by ID (resolved through the caller's preferences)",
                )
                .tag("Journeys")
                .response::<200, Json<GetResponse<Journey>>>()
                .response::<404, Json<GetErrorResponse>>()
                .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}",
            patch_with(journeys_patch_handler, |op| {
                op.description(
                    "Rename a journey from the caller's perspective. Writes a new \
                     `rev.principal_journey_preferences` row; the underlying journey is \
                     untouched and other principals' views are unaffected.",
                )
                .tag("Journeys")
                .response::<200, Json<GetResponse<Journey>>>()
                .response::<404, Json<GetErrorResponse>>()
                .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}",
            delete_with(journeys_delete_handler, |op| {
                op.description(
                    "Soft-hide a journey from the caller's lists. Appends a preferences row with \
                     `hidden_at = now()`. The journey itself, its dispatches, and other \
                     principals' views are untouched.",
                )
                .tag("Journeys")
                .response::<200, Json<GetResponse<Journey>>>()
                .response::<404, Json<DeleteErrorResponse>>()
                .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/chain",
            get_with(chain_get_handler, |op| {
                op.description(
                    "Phase 2 journey-chain: walks back through `predecessor_journey_id` \
                     from the given journey and returns the chain root→leaf in display \
                     order. Used by the dapple-ui-demo chapter resolver to drop the \
                     'latest journey per playbook' lookup that breaks under concurrent \
                     essays. Foreign/unknown ids return 404 RESOURCE_NOT_FOUND with \
                     identical bodies (no existence leak).",
                )
                .tag("Journeys")
                .response::<200, Json<IndexResponse<Journey>>>()
                .response::<404, Json<GetErrorResponse>>()
                .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/briefs",
            get_with(journeys_briefs_index_handler, |op| {
                op.description("Get all briefs for a journey with pack/group context")
                    .tag("Journeys")
                    .response::<200, Json<IndexResponse<JourneyBrief>>>()
                    .response::<422, Json<IndexErrorResponse>>()
                    .response::<500, Json<IndexErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/stages",
            get_with(journeys_stages_index_handler, |op| {
                op.description(
                    "Per-stage completion for a journey: one entry per playbook stage \
                     with `done` = true once every value slot the stage exposes (its \
                     overlays) is bound in the journey chain. A stage that authored no \
                     overlays is trivially done (strict default). Each entry also \
                     carries the authored stage `name`, resolved `description`, and \
                     optional closed-vocabulary `kind`. Authored stage order.",
                )
                .tag("Journeys")
                .response::<200, Json<IndexResponse<JourneyStageStatus>>>()
                .response::<422, Json<IndexErrorResponse>>()
                .response::<500, Json<IndexErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/steps",
            get_with(journeys_steps_index_handler, |op| {
                op.description(
                    "Per-step exposed-slot status for a journey: one entry per playbook \
                     step with `done` = true once every value slot the step exposes is \
                     bound in the journey chain. `exposed_slots` carries each slot's bind \
                     target, kind, and bound flag — finer than `/stages`, used to discover \
                     which step (e.g. a generate→pick→refine pick step) is awaiting a \
                     binding. Each entry also carries its owning stage's authored \
                     `stage_name`, resolved `stage_description`, and optional \
                     closed-vocabulary `stage_kind`, repeated identically across a \
                     stage's steps. Authored step order.",
                )
                .tag("Journeys")
                .response::<200, Json<IndexResponse<JourneyStepStatus>>>()
                .response::<422, Json<IndexErrorResponse>>()
                .response::<500, Json<IndexErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/brief-form",
            get_with(journey_brief_form_handler, |op| {
                op.description(
                    "Structural form definition for a journey (briefs, units, inputs). \
                     Cacheable via strong ETag derived from brief content_hashes; \
                     honors `If-None-Match` (returns 304 on match).",
                )
                .tag("Journeys")
                .response::<200, Json<JourneyBriefFormResponse>>()
                .response::<304, ()>()
                .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/bindings",
            get_with(journey_bindings_handler, |op| {
                op.description(
                    "Current binding state for a journey (latest binding per value_slot \
                     across all the journey's dispatches). Cheap to call; \
                     `Cache-Control: no-store`.",
                )
                .tag("Journeys")
                .response::<200, Json<JourneyBindingsResponse>>()
                .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/binding-history",
            get_with(journey_binding_history_handler, |op| {
                op.description(
                    "Every binding ever attached to any dispatch of this journey, \
                     ordered by `created_at` ASC. Sibling of `/{id}/bindings` \
                     (which collapses to the latest binding per value_slot); each \
                     row here carries its `dispatch_id` so consumers can replay \
                     answers per-dispatch.",
                )
                .tag("Journeys")
                .response::<200, Json<IndexResponse<DispatchIntentBinding>>>()
                .response::<500, Json<IndexErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/uploads",
            post_with(journey_asset_uploads_post_handler, |op| {
                op.description(
                    "Stage operator uploads for a journey (batch): a durable record \
                     that an already-created asset was uploaded for a given input \
                     before the form is submitted. The asset↔slot binding is still \
                     written at submit time via dispatch-intent-bindings; staging \
                     never fires a dispatch.",
                )
                .tag("Journeys")
                .response::<201, Json<PostResponse<JourneyAssetUpload>>>()
                .response::<400, Json<PostErrorResponse>>()
                .response::<404, Json<PostErrorResponse>>()
                .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/uploads",
            get_with(journey_asset_uploads_index_handler, |op| {
                op.description(
                    "Active (non-removed) staged uploads for a journey in creation \
                     order. UUIDv7 ids make the order chronological — the latest \
                     upload per input_key is the last matching entry. Used to \
                     rehydrate file fields after a reload and to recover asset ids \
                     at submit.",
                )
                .tag("Journeys")
                .response::<200, Json<IndexResponse<JourneyAssetUpload>>>()
                .response::<500, Json<IndexErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/uploads/{upload_id}",
            delete_with(journey_asset_upload_delete_handler, |op| {
                op.description(
                    "Soft-remove one staged upload (append-only removal record). \
                     404 when there is no active upload with that id on the journey. \
                     The underlying asset is not deleted.",
                )
                .tag("Journeys")
                .response::<200, Json<GetResponse<serde_json::Value>>>()
                .response::<404, Json<DeleteErrorResponse>>()
                .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/favorite",
            post_with(add_favorite_journey_chain_handler, |op| {
                op.description(
                    "Favorite the chain that contains this journey. The server resolves \
                     the supplied id to its chain root; favoriting any journey in a chain \
                     favorites the chain.",
                )
                .tag("Journeys")
                .tag("Favorites")
                .response::<201, Json<AddFavoriteJourneyChainResponse>>()
                .response::<200, Json<AddFavoriteJourneyChainResponse>>()
                .response::<400, Json<PostErrorResponse>>()
                .response::<409, Json<PostErrorResponse>>()
                .response::<422, Json<PostErrorResponse>>()
                .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/favorite",
            delete_with(remove_favorite_journey_chain_handler, |op| {
                op.description(
                    "Unfavorite the chain that contains this journey. Idempotent: returns \
                     200 whether or not the chain was previously favorited.",
                )
                .tag("Journeys")
                .tag("Favorites")
                .response::<200, Json<RemoveFavoriteJourneyChainResponse>>()
                .response::<404, Json<DeleteErrorResponse>>()
                .response::<409, Json<DeleteErrorResponse>>()
                .response::<422, Json<DeleteErrorResponse>>()
                .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/favorite",
            get_with(check_favorite_journey_chain_handler, |op| {
                op.description(
                    "Report whether the chain containing this journey is favorited, plus \
                     the resolved chain root id.",
                )
                .tag("Journeys")
                .tag("Favorites")
                .response::<200, Json<CheckFavoriteJourneyChainResponse>>()
                .response::<404, Json<GetErrorResponse>>()
                .response::<500, Json<GetErrorResponse>>()
            }),
        )
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("journeysIndex")
        .description("Query journeys.")
        .tag("Journeys")
        .response::<200, Json<IndexResponse<Journey>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetJourneySetSchema::apply_query_param_docs(op)
}

fn chains_index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("journeyChainsIndex")
        .description(
            "Phase 2 journey-chain summary listing for the authenticated user, \
             scoped to one guide. Returns one entry per chain root with the list \
             of leaves (branches) under it; fork-display is folded by `root_id` \
             on the client (multi-branch summaries get a branch indicator).",
        )
        .tag("Journeys")
        .response::<200, Json<IndexResponse<ChainSummary>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    ChainsIndexQuery::apply_query_param_docs(op)
}
