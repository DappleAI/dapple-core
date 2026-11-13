use crate::AppState;
use crate::handler::fluent_messages::{
    fluent_message_paths_get_handler, fluent_message_paths_index_handler,
    fluent_messages_delete_handler, fluent_messages_delete_one_handler,
    fluent_messages_get_handler, fluent_messages_index_handler, fluent_messages_patch_handler,
    fluent_messages_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::deprecations::DeleteQueryParams;
use crate::schema::fluent_messages::{GetFluentMessagePathSetSchema, GetFluentMessageSetSchema};
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, patch_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::fluent_messages::{FluentMessage, FluentMessagePath};
use std::sync::Arc;

// Router for fluent message paths (GET only - paths created with first message version)
pub fn fluent_message_paths_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            get_with(fluent_message_paths_index_handler, paths_index_docs),
        )
        .api_route(
            "/{id}",
            get_with(fluent_message_paths_get_handler, |op| {
                op.id("get_fluent_message_path")
                    .summary("Get a fluent message path by ID")
                    .description("Retrieve a specific fluent message path by object ID")
                    .tag("fluent_messages")
                    .response::<200, Json<GetResponse<FluentMessagePath>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
}

fn paths_index_docs(op: TransformOperation) -> TransformOperation {
    GetFluentMessagePathSetSchema::apply_query_param_docs(
        op.id("get_fluent_message_paths")
            .summary("Query fluent message paths")
            .description("Query fluent message paths by object IDs")
            .tag("fluent_messages")
            .response::<200, Json<GetResponse<FluentMessagePath>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}

// Router for fluent messages
pub fn fluent_messages_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(fluent_messages_post_handler, |op| {
                op.id("post_fluent_messages")
                    .summary("Create fluent messages")
                    .description(
                        "Create a batch of fluent messages with localized and engine bodies",
                    )
                    .tag("fluent_messages")
                    .response::<201, Json<PostResponse<FluentMessage>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            get_with(fluent_messages_index_handler, messages_index_docs),
        )
        .api_route(
            "/{id}",
            get_with(fluent_messages_get_handler, |op| {
                op.id("get_fluent_message")
                    .summary("Get a fluent message by ID")
                    .description("Retrieve a specific fluent message by object ID")
                    .tag("fluent_messages")
                    .response::<200, Json<GetResponse<FluentMessage>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            patch_with(fluent_messages_patch_handler, |op| {
                op.id("patch_fluent_messages")
                    .summary("Update fluent messages")
                    .description("Create new versions of existing fluent messages")
                    .tag("fluent_messages")
                    .response::<200, Json<PostResponse<FluentMessage>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            delete_with(fluent_messages_delete_handler, fluent_messages_delete_docs),
        )
        .api_route(
            "/{id}",
            delete_with(fluent_messages_delete_one_handler, |op| {
                op.id("delete_fluent_message")
                    .summary("Delete (deprecate) a fluent message")
                    .description("Soft delete a fluent message by setting its deprecation level")
                    .tag("fluent_messages")
                    .response::<200, Json<DeleteResponse<FluentMessage>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
}

fn fluent_messages_delete_docs(op: TransformOperation) -> TransformOperation {
    DeleteQueryParams::apply_query_param_docs(
        op.id("delete_fluent_messages")
            .summary("Delete (deprecate) fluent messages")
            .description(
                "Soft delete fluent messages by setting their deprecation level. \
                 Use query param `level` to specify: deprecated (default), retired, or quarantined.",
            )
            .tag("fluent_messages")
            .response::<200, Json<DeleteResponse<FluentMessage>>>()
            .response::<404, Json<DeleteErrorResponse>>()
            .response::<409, Json<DeleteErrorResponse>>()
            .response::<422, Json<DeleteErrorResponse>>()
            .response::<500, Json<DeleteErrorResponse>>(),
    )
}

fn messages_index_docs(op: TransformOperation) -> TransformOperation {
    GetFluentMessageSetSchema::apply_query_param_docs(
        op.id("get_fluent_messages")
            .summary("Query fluent messages")
            .description("Query fluent messages by object IDs or path IDs")
            .tag("fluent_messages")
            .response::<200, Json<GetResponse<FluentMessage>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}
