use crate::AppState;
use crate::handler::guide_ingest::{guide_ingest_patch_handler, guide_ingest_post_handler};
use crate::handler::guide_render::guide_render_handler;
use crate::handler::guides::{
    guide_cards_index_handler, guide_versions_handler, guides_delete_handler,
    guides_delete_one_handler, guides_get_handler, guides_index_handler, guides_post_handler,
};
use crate::handler::user_favorite_guides::{
    add_favorite_guide_handler, check_favorite_guide_handler, list_favorite_guides_handler,
    remove_favorite_guide_handler,
};
use crate::handler::{
    DeleteErrorResponse, DeleteResponse, GetErrorResponse, GetResponse, IndexErrorResponse,
    IndexResponse, PostErrorResponse, PostResponse,
};
use crate::schema::QueryParamDocs;
// Note: IngestGuideResponse, IngestValidationResult, IngestErrorResponse are used by the
// handler directly but not needed in route definitions since ingest uses plain axum routing
// (Multipart is not compatible with aide's OperationInput trait).
use crate::schema::guides::{GetGuideCardSetSchema, GetGuideSetSchema};
use crate::schema::user_favorite_guides::{
    AddFavoriteGuideResponse, CheckFavoriteGuideResponse, ListFavoriteGuidesResponse,
    RemoveFavoriteGuideResponse,
};
use aide::axum::ApiRouter;
use aide::axum::routing::{delete_with, get_with, post_with};
use aide::transform::TransformOperation;
use axum::Json;
use axum::extract::DefaultBodyLimit;
use axum::routing::patch;
use dapple_data::model::guides::{Guide, GuideCardCarouselItem, GuideVersion};
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new()
        .api_route(
            "/",
            post_with(guides_post_handler, |op| {
                op.description("Create one or more guides with chapters, blocks, and tags")
                    .tag("Guides")
                    .response::<201, Json<PostResponse<Guide>>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route("/", get_with(guides_index_handler, index_docs))
        .api_route(
            "/cards",
            get_with(guide_cards_index_handler, cards_index_docs),
        )
        .api_route(
            "/versions",
            get_with(guide_versions_handler, |op| {
                op.id("guideVersions")
                    .description(
                        "List guide version metadata (content hashes, deprecation status). \
                         Lightweight endpoint for canonical pin discovery — no chapters or blocks.",
                    )
                    .tag("Guides")
                    .response::<200, Json<IndexResponse<GuideVersion>>>()
                    .response::<422, Json<IndexErrorResponse>>()
                    .response::<500, Json<IndexErrorResponse>>()
            }),
        )
        // Ingest endpoints use Multipart which is not compatible with aide's
        // OperationInput trait, so they use standard axum routing.
        // The guide tarball (chapters + card image + PDF resources) easily exceeds
        // axum's 2 MB default body limit, so raise it to the same 256 MB ceiling the
        // assets route uses (route/assets.rs), which matches the ingress's
        // proxy-body-size: 256m annotation.
        .route(
            "/ingest",
            axum::routing::post(guide_ingest_post_handler)
                .layer(DefaultBodyLimit::max(256 * 1024 * 1024)),
        )
        .api_route(
            "/favorites",
            get_with(list_favorite_guides_handler, |op| {
                op.description("List all favorite guides for the authenticated user")
                    .tag("Guides")
                    .tag("Favorites")
                    .response::<200, Json<ListFavoriteGuidesResponse>>()
                    .response::<500, Json<IndexErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}",
            get_with(guides_get_handler, |op| {
                op.description("Retrieve a specific guide by version object ID")
                    .tag("Guides")
                    .response::<200, Json<GetResponse<Guide>>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
        .api_route(
            "/",
            delete_with(guides_delete_handler, |op| {
                op.id("delete_guides")
                    .summary("Delete guides")
                    .description("Deprecate a batch of guides by path object ID")
                    .tag("Guides")
                    .response::<200, Json<DeleteResponse<Guide>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}",
            delete_with(guides_delete_one_handler, |op| {
                op.id("delete_guide")
                    .summary("Delete a guide")
                    .description("Deprecate a single guide by path object ID")
                    .tag("Guides")
                    .response::<200, Json<DeleteResponse<Guide>>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        .route(
            "/{id}/ingest",
            patch(guide_ingest_patch_handler).layer(DefaultBodyLimit::max(256 * 1024 * 1024)),
        )
        .route("/{id}/render", axum::routing::post(guide_render_handler))
        .api_route(
            "/{id}/favorite",
            post_with(add_favorite_guide_handler, |op| {
                op.description("Add a guide to the user's favorites")
                    .tag("Guides")
                    .tag("Favorites")
                    .response::<201, Json<AddFavoriteGuideResponse>>()
                    .response::<200, Json<AddFavoriteGuideResponse>>()
                    .response::<400, Json<PostErrorResponse>>()
                    .response::<409, Json<PostErrorResponse>>()
                    .response::<422, Json<PostErrorResponse>>()
                    .response::<500, Json<PostErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/favorite",
            delete_with(remove_favorite_guide_handler, |op| {
                op.description("Remove a guide from the user's favorites")
                    .tag("Guides")
                    .tag("Favorites")
                    .response::<200, Json<RemoveFavoriteGuideResponse>>()
                    .response::<404, Json<DeleteErrorResponse>>()
                    .response::<409, Json<DeleteErrorResponse>>()
                    .response::<422, Json<DeleteErrorResponse>>()
                    .response::<500, Json<DeleteErrorResponse>>()
            }),
        )
        .api_route(
            "/{id}/favorite",
            get_with(check_favorite_guide_handler, |op| {
                op.description("Check if a guide is in the user's favorites")
                    .tag("Guides")
                    .tag("Favorites")
                    .response::<200, Json<CheckFavoriteGuideResponse>>()
                    .response::<404, Json<GetErrorResponse>>()
                    .response::<500, Json<GetErrorResponse>>()
            }),
        )
}

fn index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("guidesIndex")
        .description("Query guides.")
        .tag("Guides")
        .response::<200, Json<IndexResponse<Guide>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetGuideSetSchema::apply_query_param_docs(op)
}

fn cards_index_docs(op: TransformOperation) -> TransformOperation {
    let op = op
        .id("guideCardsIndex")
        .description("Query guide promotional cards for the card carousel. Returns the latest non-deprecated version's card for each guide path.")
        .tag("Guides")
        .tag("Cards")
        .response::<200, Json<IndexResponse<GuideCardCarouselItem>>>()
        .response::<422, Json<IndexErrorResponse>>()
        .response::<500, Json<IndexErrorResponse>>();

    GetGuideCardSetSchema::apply_query_param_docs(op)
}
