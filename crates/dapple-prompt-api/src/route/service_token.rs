//! Route definition for the service token verification endpoint.

use crate::AppState;
use crate::handler::service_token::get_service_token_handler;
use crate::schema::service_token::{GetServiceTokenResponse, ServiceTokenErrorResponse};
use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use std::sync::Arc;

pub fn router() -> ApiRouter<Arc<AppState>> {
    ApiRouter::new().api_route(
        "/",
        post_with(get_service_token_handler, |op| {
            op.summary("Get service token")
                .description(
                    "Verify an Ed25519-signed service token and issue a JWT for API access. \
                    This is a PUBLIC endpoint that does not require authentication. \
                    \n\n\
                    Service tokens use asymmetric keypairs (Ed25519). The service's public key \
                    must be registered in the database before tokens can be exchanged. \
                    \n\n\
                    Token format: 128 bytes base64 encoded (signature 64 bytes + message 64 bytes)",
                )
                .tag("Auth")
                .response::<200, Json<GetServiceTokenResponse>>()
                .response::<400, Json<ServiceTokenErrorResponse>>()
                .response::<401, Json<ServiceTokenErrorResponse>>()
                .response::<403, Json<ServiceTokenErrorResponse>>()
                .response::<404, Json<ServiceTokenErrorResponse>>()
                .response::<503, Json<ServiceTokenErrorResponse>>()
        }),
    )
}
