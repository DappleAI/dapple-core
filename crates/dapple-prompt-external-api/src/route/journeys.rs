//! Route registration for `/api/journeys` on the external surface.

use crate::ExternalAppState;
use crate::handler::journey_bindings::journey_bindings_handler;
use crate::handler::journey_brief_form::journey_brief_form_handler;
use crate::handler::journey_briefs::journey_briefs_index_handler;
use crate::handler::journey_chains::{chain_get_handler, chains_index_handler};
use crate::handler::journey_stages::journey_stages_index_handler;
use crate::handler::journey_steps::journey_steps_index_handler;
use crate::handler::journeys::{
    journeys_delete_handler, journeys_get_handler, journeys_index_handler, journeys_patch_handler,
    journeys_post_handler,
};
use aide::axum::ApiRouter;
use aide::axum::routing::{delete, get, patch, post};
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<ExternalAppState>> {
    ApiRouter::new()
        .api_route("/", post(journeys_post_handler))
        .api_route("/", get(journeys_index_handler))
        .api_route("/chains", get(chains_index_handler))
        .api_route("/{id}", get(journeys_get_handler))
        .api_route("/{id}", patch(journeys_patch_handler))
        .api_route("/{id}", delete(journeys_delete_handler))
        .api_route("/{id}/chain", get(chain_get_handler))
        .api_route("/{id}/briefs", get(journey_briefs_index_handler))
        .api_route("/{id}/stages", get(journey_stages_index_handler))
        .api_route("/{id}/steps", get(journey_steps_index_handler))
        .api_route("/{id}/brief-form", get(journey_brief_form_handler))
        .api_route("/{id}/bindings", get(journey_bindings_handler))
}
