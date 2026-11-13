use crate::AppState;
use crate::handler::assets::{
    asset_content_handler, asset_paths_get_handler, asset_paths_index_handler,
    assets_delete_handler, assets_delete_one_handler, assets_get_handler, assets_index_handler,
    assets_patch_handler, assets_post_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
use crate::schema::assets::{GetAssetPathSetSchema, GetAssetSetSchema};
use crate::schema::deprecations::DeleteQueryParams;
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, patch_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use axum::extract::DefaultBodyLimit;
use axum::routing::get;
use dapple_data::model::assets::{Asset, AssetPath};
use std::sync::Arc;

// Router for asset paths (read-only - paths are created with assets)
pub fn asset_paths_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route("/", get_with(asset_paths_index_handler, paths_index_docs))
        .api_route(
            "/{id}",
            get_with(asset_paths_get_handler, |op| {
                op.id("get_asset_path")
                    .summary("Get an asset path by ID")
                    .description("Retrieve a specific asset path by object ID")
                    .tag("assets")
                    .response::<200, Json<GetResponse<AssetPath>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
}

fn paths_index_docs(op: TransformOperation) -> TransformOperation {
    GetAssetPathSetSchema::apply_query_param_docs(
        op.id("get_asset_paths")
            .summary("Query asset paths")
            .description("Query asset paths by object IDs")
            .tag("assets")
            .response::<200, Json<GetResponse<AssetPath>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}

// Router for assets
pub fn assets_router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(assets_post_handler, |op| {
                op.id("post_assets")
                    .summary("Create assets")
                    .description("Create a batch of assets with base64-encoded contents")
                    .tag("assets")
                    .response::<201, Json<PostResponse<Asset>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(assets_index_handler, assets_index_docs))
        .api_route(
            "/{id}",
            get_with(assets_get_handler, |op| {
                op.id("get_asset")
                    .summary("Get an asset by ID")
                    .description("Retrieve a specific asset by object ID")
                    .tag("assets")
                    .response::<200, Json<GetResponse<Asset>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            patch_with(assets_patch_handler, |op| {
                op.id("patch_assets")
                    .summary("Update assets")
                    .description("Create new versions of existing assets with new contents")
                    .tag("assets")
                    .response::<200, Json<PostResponse<Asset>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", delete_with(assets_delete_handler, assets_delete_docs))
        .api_route(
            "/{id}",
            delete_with(assets_delete_one_handler, |op| {
                op.id("delete_asset")
                    .summary("Delete (deprecate) an asset")
                    .description("Soft delete an asset by setting its deprecation level")
                    .tag("assets")
                    .response::<200, Json<DeleteResponse<Asset>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        // Raw binary content streaming - not OpenAPI (binary responses don't fit JSON schema)
        .route("/{id}/content", get(asset_content_handler))
        // Operator asset uploads (e.g. a multi-minute WAV "card audio") arrive as
        // base64-encoded JSON on POST/PATCH, so a raw N-byte file becomes a
        // ~1.33*N body. Raise axum's 2 MB default request-body limit for the
        // assets router ONLY — every other JSON endpoint keeps the protective
        // default. Matches the `proxy-body-size: 256m` ingress ceiling.
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024))
}

fn assets_index_docs(op: TransformOperation) -> TransformOperation {
    GetAssetSetSchema::apply_query_param_docs(
        op.id("get_assets")
            .summary("Query assets")
            .description("Query assets by object IDs or path IDs")
            .tag("assets")
            .response::<200, Json<GetResponse<Asset>>>()
            .response::<422, Json<IndexErrorResponse>>()
            .response::<500, Json<IndexErrorResponse>>(),
    )
}

fn assets_delete_docs(op: TransformOperation) -> TransformOperation {
    DeleteQueryParams::apply_query_param_docs(
        op.id("delete_assets")
            .summary("Delete (deprecate) assets")
            .description(
                "Soft delete assets by setting their deprecation level. \
                 Use query param `level` to specify: deprecated (default), retired, or quarantined.",
            )
            .tag("assets")
            .response::<200, Json<DeleteResponse<Asset>>>()
            .response::<404, Json<DeleteErrorResponse>>()
            .response::<409, Json<DeleteErrorResponse>>()
            .response::<422, Json<DeleteErrorResponse>>()
            .response::<500, Json<DeleteErrorResponse>>(),
    )
}
