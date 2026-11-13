//! Mock utility endpoints for testing convenience.
//!
//! These endpoints are only available on the mock server and provide
//! easy access to the mock server's address and pre-signed tokens.

use crate::service::auth_token::sign_auth_token;
use crate::state::MockState;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Response for GET /mock/address
#[derive(Serialize, JsonSchema)]
pub struct MockAddressResponse {
    /// The mock server's Ethereum address (0x-prefixed)
    pub address: String,
    /// Whether authentication is enabled
    pub auth_enabled: bool,
}

/// Response for GET /mock/auth-token
#[derive(Serialize, JsonSchema)]
pub struct MockAuthTokenResponse {
    /// A pre-signed auth token valid for the specified duration
    pub token: String,
    /// The signer address (should match /mock/address)
    pub signer: String,
    /// Token validity duration in seconds
    pub valid_for_secs: u64,
}

/// Error response for mock endpoints
#[derive(Serialize, JsonSchema)]
pub struct MockErrorResponse {
    pub error: String,
    pub message: String,
}

/// Query parameters for GET /mock/auth-token
#[derive(Deserialize, Default)]
pub struct AuthTokenQuery {
    /// Token validity duration in seconds (default: 300 = 5 minutes)
    #[serde(default = "default_valid_for")]
    pub valid_for_secs: u64,
}

fn default_valid_for() -> u64 {
    300
}

/// Handler for `GET /mock/address`
///
/// Returns the mock server's Ethereum address.
/// Useful for configuring clients to accept the mock's signatures.
pub async fn mock_address_handler(
    State(state): State<Arc<MockState>>,
) -> Json<MockAddressResponse> {
    Json(MockAddressResponse {
        address: state.mock_address_hex(),
        auth_enabled: state.is_auth_enabled(),
    })
}

/// Handler for `GET /mock/auth-token`
///
/// Returns a pre-signed auth token from the mock server's keypair.
/// This is a convenience endpoint for testing - in production, clients
/// would sign their own tokens with their own keys.
pub async fn mock_auth_token_handler(
    State(state): State<Arc<MockState>>,
    Query(params): Query<AuthTokenQuery>,
) -> Result<Json<MockAuthTokenResponse>, (StatusCode, Json<MockErrorResponse>)> {
    // Enforce max duration matching production (30 minutes)
    let max_duration = 1800u64;
    if params.valid_for_secs > max_duration {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(MockErrorResponse {
                error: "DURATION_TOO_LONG".to_string(),
                message: format!("valid_for_secs cannot exceed {} seconds", max_duration),
            }),
        ));
    }

    let token = sign_auth_token(&state.keypair.signing_key, params.valid_for_secs);

    Ok(Json(MockAuthTokenResponse {
        token,
        signer: state.mock_address_hex(),
        valid_for_secs: params.valid_for_secs,
    }))
}

/// Handler for `GET /mock/status`
///
/// Returns the mock server's configuration status.
#[derive(Serialize, JsonSchema)]
pub struct MockStatusResponse {
    pub status: String,
    pub auth_enabled: bool,
    pub mock_address: String,
    pub allowed_signers_count: usize,
}

pub async fn mock_status_handler(State(state): State<Arc<MockState>>) -> Json<MockStatusResponse> {
    Json(MockStatusResponse {
        status: "ok".to_string(),
        auth_enabled: state.is_auth_enabled(),
        mock_address: state.mock_address_hex(),
        allowed_signers_count: state.auth_config.allowed_signers.len(),
    })
}
