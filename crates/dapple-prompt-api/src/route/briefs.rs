use crate::AppState;
use crate::handler::briefs::{
    brief_paths_get_handler, brief_paths_index_handler, briefs_delete_handler,
    briefs_delete_one_handler, briefs_get_handler, briefs_index_handler, briefs_patch_handler,
    briefs_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    PostErrorResponse, PostResponse,
};
use crate::route::brief_units;
use crate::schema::QueryParamDocs;
use crate::schema::briefs::{GetBriefPathSetSchema, GetBriefSetSchema};
use crate::schema::deprecations::DeleteQueryParams;
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, patch_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::briefs::{Brief, BriefPath};
use std::sync::Arc;

// Router for brief paths (read-only - paths are created with briefs)
pub fn brief_paths_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route("/", get_with(brief_paths_index_handler, paths_index_docs))
        .api_route(
            "/{id}",
            get_with(brief_paths_get_handler, |op| {
                op.id("get_brief_path")
                    .summary("Get a brief path by ID")
                    .description("Retrieve a specific brief path by object ID")
                    .tag("briefs")
                    .response::<200, Json<GetResponse<BriefPath>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
}

fn paths_index_docs(op: TransformOperation) -> TransformOperation {
    GetBriefPathSetSchema::apply_query_param_docs(
        op.id("get_brief_paths")
            .summary("Query brief paths")
            .description("Query brief paths by object IDs")
            .tag("briefs")
            .response::<200, Json<GetResponse<BriefPath>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}

// Router for briefs
pub fn briefs_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(briefs_post_handler, |op| {
                op.id("post_briefs")
                    .summary("Create briefs")
                    .description("Create a batch of briefs with titles and descriptions")
                    .tag("briefs")
                    .response::<201, Json<PostResponse<Brief>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(briefs_index_handler, briefs_index_docs))
        .api_route(
            "/{id}",
            get_with(briefs_get_handler, |op| {
                op.id("get_brief")
                    .summary("Get a brief by ID")
                    .description("Retrieve a specific brief by object ID")
                    .tag("briefs")
                    .response::<200, Json<GetResponse<Brief>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            patch_with(briefs_patch_handler, |op| {
                op.id("patch_briefs")
                    .summary("Update briefs")
                    .description("Create new versions of existing briefs")
                    .tag("briefs")
                    .response::<200, Json<PostResponse<Brief>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", delete_with(briefs_delete_handler, briefs_delete_docs))
        .api_route(
            "/{id}",
            delete_with(briefs_delete_one_handler, |op| {
                op.id("delete_brief")
                    .summary("Delete (deprecate) a brief")
                    .description("Soft delete a brief by setting its deprecation level")
                    .tag("briefs")
                    .response::<200, Json<DeleteResponse<Brief>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        // Nested router for brief units
        .nest("/{id}/units/", brief_units::router())
}

fn briefs_index_docs(op: TransformOperation) -> TransformOperation {
    GetBriefSetSchema::apply_query_param_docs(
        op.id("get_briefs")
            .summary("Query briefs")
            .description("Query briefs by object IDs or path IDs")
            .tag("briefs")
            .response::<200, Json<GetResponse<Brief>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}

fn briefs_delete_docs(op: TransformOperation) -> TransformOperation {
    DeleteQueryParams::apply_query_param_docs(
        op.id("delete_briefs")
            .summary("Delete (deprecate) briefs")
            .description(
                "Soft delete briefs by setting their deprecation level. \
                 Use query param `level` to specify: deprecated (default), retired, or quarantined.",
            )
            .tag("briefs")
            .response::<200, Json<DeleteResponse<Brief>>>()
            .response::<404, Json<DeleteErrorResponse>>()
            .response::<409, Json<DeleteErrorResponse>>()
            .response::<422, Json<DeleteErrorResponse>>()
            .response::<500, Json<DeleteErrorResponse>>(),
    )
}
