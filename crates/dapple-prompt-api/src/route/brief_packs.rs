use crate::AppState;
use crate::handler::brief_packs::{
    brief_pack_paths_get_handler, brief_pack_paths_index_handler, brief_packs_delete_handler,
    brief_packs_delete_one_handler, brief_packs_get_handler, brief_packs_index_handler,
    brief_packs_patch_handler, brief_packs_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::brief_packs::{GetBriefPackPathSetSchema, GetBriefPackSetSchema};
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, patch_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use dapple_data::model::brief_packs::{BriefPack, BriefPackPath};
use std::sync::Arc;

// Router for brief pack paths (read-only - paths are created with brief packs)
pub fn brief_pack_paths_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            get_with(brief_pack_paths_index_handler, paths_index_docs),
        )
        .api_route(
            "/{id}",
            get_with(brief_pack_paths_get_handler, |op| {
                op.id("get_brief_pack_path")
                    .summary("Get a brief pack path by ID")
                    .description("Retrieve a specific brief pack path by object ID")
                    .tag("brief_packs")
                    .response::<200, Json<GetResponse<BriefPackPath>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
}

fn paths_index_docs(op: TransformOperation) -> TransformOperation {
    GetBriefPackPathSetSchema::apply_query_param_docs(
        op.id("get_brief_pack_paths")
            .summary("Query brief pack paths")
            .description("Query brief pack paths by object IDs")
            .tag("brief_packs")
            .response::<200, Json<GetResponse<BriefPackPath>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}

// Router for brief packs
pub fn brief_packs_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(brief_packs_post_handler, |op| {
                op.id("post_brief_packs")
                    .summary("Create brief packs")
                    .description("Create a batch of brief packs with associated groups")
                    .tag("brief_packs")
                    .response::<201, Json<PostResponse<BriefPack>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            get_with(brief_packs_index_handler, brief_packs_index_docs),
        )
        .api_route(
            "/{id}",
            get_with(brief_packs_get_handler, |op| {
                op.id("get_brief_pack")
                    .summary("Get a brief pack by ID")
                    .description("Retrieve a specific brief pack by object ID")
                    .tag("brief_packs")
                    .response::<200, Json<GetResponse<BriefPack>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            patch_with(brief_packs_patch_handler, |op| {
                op.id("patch_brief_packs")
                    .summary("Update brief packs")
                    .description("Create new versions of existing brief packs")
                    .tag("brief_packs")
                    .response::<200, Json<PostResponse<BriefPack>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            delete_with(brief_packs_delete_handler, |op| {
                op.id("delete_brief_packs")
                    .summary("Delete brief packs")
                    .description("Deprecate a batch of brief packs by path object ID")
                    .tag("brief_packs")
                    .response::<200, Json<DeleteResponse<BriefPack>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}",
            delete_with(brief_packs_delete_one_handler, |op| {
                op.id("delete_brief_pack")
                    .summary("Delete a brief pack")
                    .description("Deprecate a single brief pack by path object ID")
                    .tag("brief_packs")
                    .response::<200, Json<DeleteResponse<BriefPack>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
}

fn brief_packs_index_docs(op: TransformOperation) -> TransformOperation {
    GetBriefPackSetSchema::apply_query_param_docs(
        op.id("get_brief_packs")
            .summary("Query brief packs")
            .description("Query brief packs by object IDs or path IDs")
            .tag("brief_packs")
            .response::<200, Json<GetResponse<BriefPack>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}
