//! Route definition for the auth token verification endpoint.

use crate::AppState;
use crate::handler::auth_token::get_auth_token_handler;
use crate::schema::auth_token::{AuthTokenErrorResponse, GetAuthTokenResponse};
use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new().api_route(
        "/",
        post_with(get_auth_token_handler, |op| {
            op.summary("Get auth token")
                .description(
                    "Verify an Ethereum-signed auth token and issue a JWT for API access. \
                    This is a PUBLIC endpoint that does not require authentication.",
                )
                .tag("Auth")
                .response::<200, Json<GetAuthTokenResponse>>()
                .response::<400, Json<AuthTokenErrorResponse>>()
                .response::<401, Json<AuthTokenErrorResponse>>()
                .response::<403, Json<AuthTokenErrorResponse>>()
                .response::<503, Json<AuthTokenErrorResponse>>()
        }),
    )
}
