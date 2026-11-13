use crate::AppState;
use crate::handler::guide_search::guide_search_handler;
use crate::handler::{IndexErrorResponse, IndexResponse};
use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use dapple_data::model::guide_content_embeddings::GuideSearchResult;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new().api_route(
        "/",
        get_with(guide_search_handler, |op| {
            op.id("guideSearch")
                .description(
                    "Search guide content using natural language. Embeds the query \
                     via Conduit's embedding service, then runs pgvector similarity \
                     search with audience tag and locale filtering.",
                )
                .tag("Search")
                .tag("Guides")
                .response::<200, Json<IndexResponse<GuideSearchResult>>>()
                .response::<400, Json<IndexErrorResponse>>()
                .response::<503, Json<IndexErrorResponse>>()
                .response::<500, Json<IndexErrorResponse>>()
        }),
    )
}
