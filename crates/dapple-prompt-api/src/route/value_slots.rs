use crate::AppState;
use crate::handler::value_slots::{
    value_slot_paths_get_handler, value_slot_paths_index_handler, value_slots_delete_handler,
    value_slots_delete_one_handler, value_slots_get_handler, value_slots_index_handler,
    value_slots_patch_handler, value_slots_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::deprecations::DeleteQueryParams;
use crate::schema::value_slots::{GetValueSlotPathSetSchema, GetValueSlotSetSchema};
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, patch_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::value_slots::{ValueSlot, ValueSlotPath};
use std::sync::Arc;

// Router for value slot paths (read-only - paths are created with value slots)
pub fn value_slot_paths_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            get_with(value_slot_paths_index_handler, paths_index_docs),
        )
        .api_route(
            "/{id}",
            get_with(value_slot_paths_get_handler, |op| {
                op.id("get_value_slot_path")
                    .summary("Get a value slot path by ID")
                    .description("Retrieve a specific value slot path by object ID")
                    .tag("value_slots")
                    .response::<200, Json<GetResponse<ValueSlotPath>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
}

fn paths_index_docs(op: TransformOperation) -> TransformOperation {
    GetValueSlotPathSetSchema::apply_query_param_docs(
        op.id("get_value_slot_paths")
            .summary("Query value slot paths")
            .description("Query value slot paths by object IDs")
            .tag("value_slots")
            .response::<200, Json<GetResponse<ValueSlotPath>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}

// Router for value slots
pub fn value_slots_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(value_slots_post_handler, |op| {
                op.id("post_value_slots")
                    .summary("Create value slots")
                    .description("Create a batch of value slots with titles and constraints")
                    .tag("value_slots")
                    .response::<201, Json<PostResponse<ValueSlot>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(value_slots_index_handler, slots_index_docs))
        .api_route(
            "/{id}",
            get_with(value_slots_get_handler, |op| {
                op.id("get_value_slot")
                    .summary("Get a value slot by ID")
                    .description("Retrieve a specific value slot by object ID")
                    .tag("value_slots")
                    .response::<200, Json<GetResponse<ValueSlot>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            patch_with(value_slots_patch_handler, |op| {
                op.id("patch_value_slots")
                    .summary("Update value slots")
                    .description("Create new versions of existing value slots")
                    .tag("value_slots")
                    .response::<200, Json<PostResponse<ValueSlot>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            delete_with(value_slots_delete_handler, value_slots_delete_docs),
        )
        .api_route(
            "/{id}",
            delete_with(value_slots_delete_one_handler, |op| {
                op.id("delete_value_slot")
                    .summary("Delete (deprecate) a value slot")
                    .description("Soft delete a value slot by setting its deprecation level")
                    .tag("value_slots")
                    .response::<200, Json<DeleteResponse<ValueSlot>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
}

fn value_slots_delete_docs(op: TransformOperation) -> TransformOperation {
    DeleteQueryParams::apply_query_param_docs(
        op.id("delete_value_slots")
            .summary("Delete (deprecate) value slots")
            .description(
                "Soft delete value slots by setting their deprecation level. \
                 Use query param `level` to specify: deprecated (default), retired, or quarantined.",
            )
            .tag("value_slots")
            .response::<200, Json<DeleteResponse<ValueSlot>>>()
            .response::<404, Json<DeleteErrorResponse>>()
            .response::<409, Json<DeleteErrorResponse>>()
            .response::<422, Json<DeleteErrorResponse>>()
            .response::<500, Json<DeleteErrorResponse>>(),
    )
}

fn slots_index_docs(op: TransformOperation) -> TransformOperation {
    GetValueSlotSetSchema::apply_query_param_docs(
        op.id("get_value_slots")
            .summary("Query value slots")
            .description("Query value slots by object IDs or path IDs")
            .tag("value_slots")
            .response::<200, Json<GetResponse<ValueSlot>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}
