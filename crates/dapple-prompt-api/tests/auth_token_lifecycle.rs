//! Integration tests for the auth token verification endpoint.
//!
//! These tests exercise the `/api/get-auth-token` endpoint end-to-end,
//! verifying that Ethereum-signed tokens are properly validated and
//! that JWTs are correctly issued.
//!
//! ## Running These Tests
//!
//! ```bash
//! cargo test -p dapple-prompt-api --test auth-token-lifecycle --features test-helpers
//! ```
//!
//! ## Note on Test Isolation
//!
//! These tests use environment variables for configuration. Since env vars are
//! process-global, tests are marked with `#[serial]` to prevent race conditions
//! when multiple tests modify the same env vars.

use dapple_prompt_api::schema::auth_token::{
    AuthTokenErrorResponse, GetAuthTokenRequest, GetAuthTokenResponse,
};
use dapple_prompt_api::test_utils::TestKeypair;
use reqwest::Client;
use serial_test::serial;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

mod consumer;
use consumer::spawn_app;

/// Helper to set environment variables for a test scope.
/// Uses unsafe because env vars are process-global state.
struct EnvGuard {
    key: String,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        Self {
            key: key.to_string(),
            previous,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(ref prev) = self.previous {
                std::env::set_var(&self.key, prev);
            } else {
                std::env::remove_var(&self.key);
            }
        }
    }
}

#[tokio::test]
#[serial]
async fn test_valid_token_returns_jwt() {
    // Generate test keypair
    let keypair = TestKeypair::random();

    // Set up environment
    let _jwt_guard = EnvGuard::set(
        "DAPPLE_PROMPT_API_SIGNING_KEY",
        "test-secret-for-jwt-signing",
    );
    let _signers_guard = EnvGuard::set("AUTH_TOKEN_ALLOWED_SIGNERS", &keypair.address_hex());

    // Spawn the app
    let (base_url, _handle) = spawn_app().await;

    // Create a valid token
    let token = keypair.sign_token(300); // 5 minutes validity
    let user_id = Uuid::now_v7();

    // Make request
    let client = Client::new();
    let response = client
        .post(format!("{}/api/get-auth-token/", base_url))
        .json(&GetAuthTokenRequest { token, user_id })
        .send()
        .await
        .expect("request failed");

    assert_eq!(response.status(), 200, "expected 200 OK");

    let body: GetAuthTokenResponse = response.json().await.expect("failed to parse response");
    assert_eq!(body.status, "ok");
    assert!(!body.jwt.is_empty(), "JWT should not be empty");
    assert_eq!(
        body.signer.to_lowercase(),
        keypair.address_hex().to_lowercase()
    );
}

#[tokio::test]
#[serial]
async fn test_unauthorized_signer_returns_403() {
    // Generate two keypairs - sign with one, allow only the other
    let signing_keypair = TestKeypair::random();
    let allowed_keypair = TestKeypair::random();

    // Set up environment with different allowed signer
    let _jwt_guard = EnvGuard::set(
        "DAPPLE_PROMPT_API_SIGNING_KEY",
        "test-secret-for-jwt-signing",
    );
    let _signers_guard =
        EnvGuard::set("AUTH_TOKEN_ALLOWED_SIGNERS", &allowed_keypair.address_hex());

    let (base_url, _handle) = spawn_app().await;

    // Sign token with the unauthorized keypair
    let token = signing_keypair.sign_token(300);
    let user_id = Uuid::now_v7();

    let client = Client::new();
    let response = client
        .post(format!("{}/api/get-auth-token/", base_url))
        .json(&GetAuthTokenRequest { token, user_id })
        .send()
        .await
        .expect("request failed");

    assert_eq!(response.status(), 403, "expected 403 Forbidden");

    let body: AuthTokenErrorResponse = response.json().await.expect("failed to parse response");
    assert_eq!(body.error, "SIGNER_NOT_AUTHORIZED");
}

#[tokio::test]
#[serial]
async fn test_expired_token_returns_401() {
    let keypair = TestKeypair::random();

    let _jwt_guard = EnvGuard::set(
        "DAPPLE_PROMPT_API_SIGNING_KEY",
        "test-secret-for-jwt-signing",
    );
    let _signers_guard = EnvGuard::set("AUTH_TOKEN_ALLOWED_SIGNERS", &keypair.address_hex());

    let (base_url, _handle) = spawn_app().await;

    // Create an expired token (valid_from and valid_until in the past)
    // Duration must be within MAX_DURATION_SECS (1800s / 30 min)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let token = keypair.sign_token_with_times(now - 600, now - 300); // 5 min duration, expired 5 min ago
    let user_id = Uuid::now_v7();

    let client = Client::new();
    let response = client
        .post(format!("{}/api/get-auth-token/", base_url))
        .json(&GetAuthTokenRequest { token, user_id })
        .send()
        .await
        .expect("request failed");

    assert_eq!(response.status(), 401, "expected 401 Unauthorized");

    let body: AuthTokenErrorResponse = response.json().await.expect("failed to parse response");
    assert_eq!(body.error, "EXPIRED");
}

#[tokio::test]
#[serial]
async fn test_not_yet_valid_token_returns_401() {
    let keypair = TestKeypair::random();

    let _jwt_guard = EnvGuard::set(
        "DAPPLE_PROMPT_API_SIGNING_KEY",
        "test-secret-for-jwt-signing",
    );
    let _signers_guard = EnvGuard::set("AUTH_TOKEN_ALLOWED_SIGNERS", &keypair.address_hex());

    let (base_url, _handle) = spawn_app().await;

    // Create a token that becomes valid in the future
    // Duration must be within MAX_DURATION_SECS (1800s / 30 min)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let token = keypair.sign_token_with_times(now + 300, now + 600); // 5 min duration, valid in 5 min
    let user_id = Uuid::now_v7();

    let client = Client::new();
    let response = client
        .post(format!("{}/api/get-auth-token/", base_url))
        .json(&GetAuthTokenRequest { token, user_id })
        .send()
        .await
        .expect("request failed");

    assert_eq!(response.status(), 401, "expected 401 Unauthorized");

    let body: AuthTokenErrorResponse = response.json().await.expect("failed to parse response");
    assert_eq!(body.error, "NOT_YET_VALID");
}

#[tokio::test]
#[serial]
async fn test_malformed_token_returns_400() {
    let keypair = TestKeypair::random();

    let _jwt_guard = EnvGuard::set(
        "DAPPLE_PROMPT_API_SIGNING_KEY",
        "test-secret-for-jwt-signing",
    );
    let _signers_guard = EnvGuard::set("AUTH_TOKEN_ALLOWED_SIGNERS", &keypair.address_hex());

    let (base_url, _handle) = spawn_app().await;

    // Send garbage token
    let user_id = Uuid::now_v7();

    let client = Client::new();
    let response = client
        .post(format!("{}/api/get-auth-token/", base_url))
        .json(&GetAuthTokenRequest {
            token: "not-valid-base64!!!".to_string(),
            user_id,
        })
        .send()
        .await
        .expect("request failed");

    assert_eq!(response.status(), 400, "expected 400 Bad Request");

    let body: AuthTokenErrorResponse = response.json().await.expect("failed to parse response");
    assert_eq!(body.error, "MALFORMED_TOKEN");
}

#[tokio::test]
#[serial]
async fn test_wrong_length_token_returns_400() {
    let keypair = TestKeypair::random();

    let _jwt_guard = EnvGuard::set(
        "DAPPLE_PROMPT_API_SIGNING_KEY",
        "test-secret-for-jwt-signing",
    );
    let _signers_guard = EnvGuard::set("AUTH_TOKEN_ALLOWED_SIGNERS", &keypair.address_hex());

    let (base_url, _handle) = spawn_app().await;

    // Send token with wrong length (valid base64 but wrong byte count)
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    let wrong_length_token = BASE64.encode(vec![0u8; 100]); // Should be 129 bytes
    let user_id = Uuid::now_v7();

    let client = Client::new();
    let response = client
        .post(format!("{}/api/get-auth-token/", base_url))
        .json(&GetAuthTokenRequest {
            token: wrong_length_token,
            user_id,
        })
        .send()
        .await
        .expect("request failed");

    assert_eq!(response.status(), 400, "expected 400 Bad Request");

    let body: AuthTokenErrorResponse = response.json().await.expect("failed to parse response");
    assert_eq!(body.error, "MALFORMED_TOKEN");
}

#[tokio::test]
#[serial]
async fn test_missing_config_returns_503() {
    // Ensure config is NOT set
    unsafe {
        std::env::remove_var("DAPPLE_PROMPT_API_SIGNING_KEY");
        std::env::remove_var("AUTH_TOKEN_ALLOWED_SIGNERS");
    }

    let (base_url, _handle) = spawn_app().await;

    let keypair = TestKeypair::random();
    let token = keypair.sign_token(300);
    let user_id = Uuid::now_v7();

    let client = Client::new();
    let response = client
        .post(format!("{}/api/get-auth-token/", base_url))
        .json(&GetAuthTokenRequest { token, user_id })
        .send()
        .await
        .expect("request failed");

    assert_eq!(response.status(), 503, "expected 503 Service Unavailable");

    let body: AuthTokenErrorResponse = response.json().await.expect("failed to parse response");
    assert_eq!(body.error, "SERVICE_UNAVAILABLE");
}

#[tokio::test]
#[serial]
async fn test_duration_exceeds_max_returns_400() {
    let keypair = TestKeypair::random();

    let _jwt_guard = EnvGuard::set(
        "DAPPLE_PROMPT_API_SIGNING_KEY",
        "test-secret-for-jwt-signing",
    );
    let _signers_guard = EnvGuard::set("AUTH_TOKEN_ALLOWED_SIGNERS", &keypair.address_hex());

    let (base_url, _handle) = spawn_app().await;

    // Create a token with duration > MAX_DURATION_SECS (1800)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let token = keypair.sign_token_with_times(now, now + 3600); // 1 hour > 30 min max
    let user_id = Uuid::now_v7();

    let client = Client::new();
    let response = client
        .post(format!("{}/api/get-auth-token/", base_url))
        .json(&GetAuthTokenRequest { token, user_id })
        .send()
        .await
        .expect("request failed");

    assert_eq!(response.status(), 400, "expected 400 Bad Request");

    let body: AuthTokenErrorResponse = response.json().await.expect("failed to parse response");
    assert_eq!(body.error, "DURATION_EXCEEDS_MAX");
}

#[tokio::test]
#[serial]
async fn test_multiple_allowed_signers() {
    // Test that multiple signers can be configured
    let keypair1 = TestKeypair::random();
    let keypair2 = TestKeypair::random();
    let keypair3 = TestKeypair::random();

    // Allow keypair1 and keypair2, but not keypair3
    let allowed = format!("{},{}", keypair1.address_hex(), keypair2.address_hex());

    let _jwt_guard = EnvGuard::set(
        "DAPPLE_PROMPT_API_SIGNING_KEY",
        "test-secret-for-jwt-signing",
    );
    let _signers_guard = EnvGuard::set("AUTH_TOKEN_ALLOWED_SIGNERS", &allowed);

    let (base_url, _handle) = spawn_app().await;
    let client = Client::new();

    // keypair1 should succeed
    let token1 = keypair1.sign_token(300);
    let response1 = client
        .post(format!("{}/api/get-auth-token/", base_url))
        .json(&GetAuthTokenRequest {
            token: token1,
            user_id: Uuid::now_v7(),
        })
        .send()
        .await
        .expect("request failed");
    assert_eq!(response1.status(), 200, "keypair1 should be allowed");

    // keypair2 should succeed
    let token2 = keypair2.sign_token(300);
    let response2 = client
        .post(format!("{}/api/get-auth-token/", base_url))
        .json(&GetAuthTokenRequest {
            token: token2,
            user_id: Uuid::now_v7(),
        })
        .send()
        .await
        .expect("request failed");
    assert_eq!(response2.status(), 200, "keypair2 should be allowed");

    // keypair3 should fail
    let token3 = keypair3.sign_token(300);
    let response3 = client
        .post(format!("{}/api/get-auth-token/", base_url))
        .json(&GetAuthTokenRequest {
            token: token3,
            user_id: Uuid::now_v7(),
        })
        .send()
        .await
        .expect("request failed");
    assert_eq!(response3.status(), 403, "keypair3 should be forbidden");
}
