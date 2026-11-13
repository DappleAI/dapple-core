use crate::handler;
use crate::state::MockState;
use aide::openapi::OpenApi;
use aide::redoc::Redoc;
use axum::response::Html;
use axum::routing::{get, patch, post};
use axum::{Extension, Json, Router};
use std::sync::Arc;

/// Handler for serving the OpenAPI JSON spec
async fn serve_api_json(Extension(api): Extension<OpenApi>) -> Json<OpenApi> {
    Json(api)
}

/// Handler for serving the Redoc documentation UI
async fn serve_docs() -> Html<String> {
    let html = Redoc::new("/api.json").html();
    Html(html)
}

/// Build the complete router mirroring production routes
pub fn build_router(state: Arc<MockState>) -> Router {
    // Log the mock's address on startup
    tracing::info!(
        "Mock server address: {} (auth_enabled: {})",
        state.mock_address_hex(),
        state.is_auth_enabled()
    );

    Router::new()
        // Documentation routes (reused from real API)
        .route("/api.json", get(serve_api_json))
        .route("/docs", get(serve_docs))
        // Health check
        .route("/api/health_check", get(handler::health_checker_handler))
        // Auth token endpoint (matches production)
        .route(
            "/api/get-auth-token",
            post(handler::auth_token::get_auth_token_handler),
        )
        // Mock utility endpoints (test convenience)
        .nest("/mock", mock_utils_router())
        // Simple entities
        .nest("/api/categories", categories_router())
        .nest("/api/hats", hats_router())
        .nest("/api/tasks", tasks_router())
        // Dependent entities
        .nest("/api/steps", steps_router())
        .nest("/api/stages", stages_router())
        .nest("/api/playbooks", playbooks_router())
        .nest("/api/journeys", journeys_router())
        // Dispatches with nested routes
        .nest("/api/dispatches", dispatches_router())
        // Versioned entities
        .nest("/api/value-slot-paths", value_slot_paths_router())
        .nest("/api/value-slots", value_slots_router())
        .nest("/api/asset-paths", asset_paths_router())
        .nest("/api/assets", assets_router())
        .nest("/api/fluent-message-paths", fluent_message_paths_router())
        .nest("/api/fluent-messages", fluent_messages_router())
        .nest("/api/brief-paths", brief_paths_router())
        .nest("/api/briefs", briefs_router())
        // Associative entities
        .nest("/api/brief-group-paths", brief_group_paths_router())
        .nest("/api/brief-groups", brief_groups_router())
        .nest("/api/brief-pack-paths", brief_pack_paths_router())
        .nest("/api/brief-packs", brief_packs_router())
        .nest("/api/prompt-paths", prompt_paths_router())
        .nest("/api/prompts", prompts_router())
        .nest("/api/task-prompts", task_prompts_router())
        .nest(
            "/api/dispatch-intent-bindings",
            dispatch_intent_bindings_router(),
        )
        .with_state(state)
}

fn categories_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::categories::categories_index_handler))
        .route("/", post(handler::categories::categories_post_handler))
        .route("/{id}", get(handler::categories::categories_get_handler))
}

fn hats_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::hats::hats_index_handler))
        .route("/", post(handler::hats::hats_post_handler))
        .route("/{id}", get(handler::hats::hats_get_handler))
}

fn tasks_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::tasks::tasks_index_handler))
        .route("/", post(handler::tasks::tasks_post_handler))
        .route("/{id}", get(handler::tasks::tasks_get_handler))
}

fn steps_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::steps::steps_index_handler))
        .route("/", post(handler::steps::steps_post_handler))
        .route("/{id}", get(handler::steps::steps_get_handler))
}

fn stages_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::stages::stages_index_handler))
        .route("/", post(handler::stages::stages_post_handler))
        .route("/{id}", get(handler::stages::stages_get_handler))
}

fn playbooks_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::playbooks::playbooks_index_handler))
        .route("/", post(handler::playbooks::playbooks_post_handler))
        .route("/", patch(handler::playbooks::playbooks_patch_handler))
        .route("/{id}", get(handler::playbooks::playbooks_get_handler))
}

fn journeys_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::journeys::journeys_index_handler))
        .route("/", post(handler::journeys::journeys_post_handler))
        .route("/{id}", get(handler::journeys::journeys_get_handler))
}

fn dispatches_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::dispatches::dispatches_index_handler))
        .route("/", post(handler::dispatches::dispatches_post_handler))
        .route("/{id}", get(handler::dispatches::dispatches_get_handler))
        .route("/phases", get(handler::dispatches::dispatch_phases_handler))
        // Nested task attempts
        .route(
            "/{dispatch_id}/task_attempt/",
            get(handler::dispatches::task_attempts_index_handler),
        )
        .route(
            "/{dispatch_id}/task_attempt/{attempt_id}",
            get(handler::dispatches::task_attempts_get_handler),
        )
}

fn value_slots_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::value_slots::value_slots_index_handler))
        .route("/", post(handler::value_slots::value_slots_post_handler))
        .route("/", patch(handler::value_slots::value_slots_patch_handler))
        .route("/{id}", get(handler::value_slots::value_slots_get_handler))
}

fn value_slot_paths_router() -> Router<Arc<MockState>> {
    Router::new()
        .route(
            "/",
            get(handler::value_slots::value_slot_paths_index_handler),
        )
        .route(
            "/{id}",
            get(handler::value_slots::value_slot_paths_get_handler),
        )
}

fn assets_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::assets::assets_index_handler))
        .route("/", post(handler::assets::assets_post_handler))
        .route("/", patch(handler::assets::assets_patch_handler))
        .route("/{id}", get(handler::assets::assets_get_handler))
}

fn asset_paths_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::assets::asset_paths_index_handler))
        .route("/{id}", get(handler::assets::asset_paths_get_handler))
}

fn fluent_messages_router() -> Router<Arc<MockState>> {
    Router::new()
        .route(
            "/",
            get(handler::fluent_messages::fluent_messages_index_handler),
        )
        .route(
            "/",
            post(handler::fluent_messages::fluent_messages_post_handler),
        )
        .route(
            "/",
            patch(handler::fluent_messages::fluent_messages_patch_handler),
        )
        .route(
            "/{id}",
            get(handler::fluent_messages::fluent_messages_get_handler),
        )
}

fn fluent_message_paths_router() -> Router<Arc<MockState>> {
    Router::new()
        .route(
            "/",
            get(handler::fluent_messages::fluent_message_paths_index_handler),
        )
        .route(
            "/{id}",
            get(handler::fluent_messages::fluent_message_paths_get_handler),
        )
}

fn briefs_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::briefs::briefs_index_handler))
        .route("/", post(handler::briefs::briefs_post_handler))
        .route("/", patch(handler::briefs::briefs_patch_handler))
        .route("/{id}", get(handler::briefs::briefs_get_handler))
        // Nested brief units
        .route(
            "/{brief_id}/units/",
            get(handler::brief_units::brief_units_index_handler),
        )
        .route(
            "/{brief_id}/units/",
            post(handler::brief_units::brief_units_post_handler),
        )
        .route(
            "/{brief_id}/units/",
            patch(handler::brief_units::brief_units_patch_handler),
        )
        .route(
            "/{brief_id}/units/{unit_id}",
            get(handler::brief_units::brief_units_get_handler),
        )
}

fn brief_paths_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::briefs::brief_paths_index_handler))
        .route("/{id}", get(handler::briefs::brief_paths_get_handler))
}

fn brief_groups_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::brief_groups::brief_groups_index_handler))
        .route("/", post(handler::brief_groups::brief_groups_post_handler))
        .route(
            "/",
            patch(handler::brief_groups::brief_groups_patch_handler),
        )
        .route(
            "/{id}",
            get(handler::brief_groups::brief_groups_get_handler),
        )
}

fn brief_group_paths_router() -> Router<Arc<MockState>> {
    Router::new()
        .route(
            "/",
            get(handler::brief_groups::brief_group_paths_index_handler),
        )
        .route(
            "/{id}",
            get(handler::brief_groups::brief_group_paths_get_handler),
        )
}

fn brief_packs_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::brief_packs::brief_packs_index_handler))
        .route("/", post(handler::brief_packs::brief_packs_post_handler))
        .route("/", patch(handler::brief_packs::brief_packs_patch_handler))
        .route("/{id}", get(handler::brief_packs::brief_packs_get_handler))
}

fn brief_pack_paths_router() -> Router<Arc<MockState>> {
    Router::new()
        .route(
            "/",
            get(handler::brief_packs::brief_pack_paths_index_handler),
        )
        .route(
            "/{id}",
            get(handler::brief_packs::brief_pack_paths_get_handler),
        )
}

fn prompts_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::prompts::prompts_index_handler))
        .route("/", post(handler::prompts::prompts_post_handler))
        .route("/", patch(handler::prompts::prompts_patch_handler))
        .route("/{id}", get(handler::prompts::prompts_get_handler))
}

fn prompt_paths_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::prompts::prompt_paths_index_handler))
        .route("/{id}", get(handler::prompts::prompt_paths_get_handler))
}

fn task_prompts_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/", get(handler::task_prompts::task_prompts_index_handler))
        .route("/", post(handler::task_prompts::task_prompts_post_handler))
        .route(
            "/{id}",
            get(handler::task_prompts::task_prompts_get_handler),
        )
}

fn dispatch_intent_bindings_router() -> Router<Arc<MockState>> {
    Router::new()
        .route(
            "/",
            get(handler::dispatch_intent_bindings::dispatch_intent_bindings_index_handler),
        )
        .route(
            "/",
            post(handler::dispatch_intent_bindings::dispatch_intent_bindings_post_handler),
        )
        .route(
            "/{id}",
            get(handler::dispatch_intent_bindings::dispatch_intent_bindings_get_handler),
        )
}

/// Mock utility routes for testing convenience
fn mock_utils_router() -> Router<Arc<MockState>> {
    Router::new()
        .route("/address", get(handler::mock_utils::mock_address_handler))
        .route(
            "/auth-token",
            get(handler::mock_utils::mock_auth_token_handler),
        )
        .route("/status", get(handler::mock_utils::mock_status_handler))
}
