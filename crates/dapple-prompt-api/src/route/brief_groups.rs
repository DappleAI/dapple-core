use crate::AppState;
use crate::handler::brief_groups::{
    brief_group_paths_get_handler, brief_group_paths_index_handler, brief_groups_delete_handler,
    brief_groups_delete_one_handler, brief_groups_get_handler, brief_groups_index_handler,
    brief_groups_patch_handler, brief_groups_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::brief_groups::{GetBriefGroupPathSetSchema, GetBriefGroupSetSchema};
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, patch_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::brief_groups::{BriefGroup, BriefGroupPath};
use std::sync::Arc;

// Router for brief group paths (read-only - paths are created with brief groups)
pub fn brief_group_paths_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            get_with(brief_group_paths_index_handler, paths_index_docs),
        )
        .api_route(
            "/{id}",
            get_with(brief_group_paths_get_handler, |op| {
                op.id("get_brief_group_path")
                    .summary("Get a brief group path by ID")
                    .description("Retrieve a specific brief group path by object ID")
                    .tag("brief_groups")
                    .response::<200, Json<GetResponse<BriefGroupPath>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
}

fn paths_index_docs(op: TransformOperation) -> TransformOperation {
    GetBriefGroupPathSetSchema::apply_query_param_docs(
        op.id("get_brief_group_paths")
            .summary("Query brief group paths")
            .description("Query brief group paths by object IDs")
            .tag("brief_groups")
            .response::<200, Json<GetResponse<BriefGroupPath>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}

// Router for brief groups
pub fn brief_groups_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(brief_groups_post_handler, |op| {
                op.id("post_brief_groups")
                    .summary("Create brief groups")
                    .description("Create a batch of brief groups with titles and associated briefs")
                    .tag("brief_groups")
                    .response::<201, Json<PostResponse<BriefGroup>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            get_with(brief_groups_index_handler, brief_groups_index_docs),
        )
        .api_route(
            "/{id}",
            get_with(brief_groups_get_handler, |op| {
                op.id("get_brief_group")
                    .summary("Get a brief group by ID")
                    .description("Retrieve a specific brief group by object ID")
                    .tag("brief_groups")
                    .response::<200, Json<GetResponse<BriefGroup>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            patch_with(brief_groups_patch_handler, |op| {
                op.id("patch_brief_groups")
                    .summary("Update brief groups")
                    .description("Create new versions of existing brief groups")
                    .tag("brief_groups")
                    .response::<200, Json<PostResponse<BriefGroup>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            delete_with(brief_groups_delete_handler, |op| {
                op.id("delete_brief_groups")
                    .summary("Delete brief groups")
                    .description("Deprecate a batch of brief groups by path object ID")
                    .tag("brief_groups")
                    .response::<200, Json<DeleteResponse<BriefGroup>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}",
            delete_with(brief_groups_delete_one_handler, |op| {
                op.id("delete_brief_group")
                    .summary("Delete a brief group")
                    .description("Deprecate a single brief group by path object ID")
                    .tag("brief_groups")
                    .response::<200, Json<DeleteResponse<BriefGroup>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
}

fn brief_groups_index_docs(op: TransformOperation) -> TransformOperation {
    GetBriefGroupSetSchema::apply_query_param_docs(
        op.id("get_brief_groups")
            .summary("Query brief groups")
            .description("Query brief groups by object IDs or path IDs")
            .tag("brief_groups")
            .response::<200, Json<GetResponse<BriefGroup>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}
