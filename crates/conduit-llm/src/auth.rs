//! Internal authentication middleware.
//!
//! Validates that requests include a valid X-Internal-Key header matching
//! CONDUIT_INTERNAL_KEY. Health check endpoints are exempt.

use axum::{body::Body, extract::Request, http::StatusCode, middleware::Next, response::Response};

/// Extension type to carry the expected internal key into the middleware.
#[derive(Clone)]
pub struct InternalKey(pub String);

/// Middleware that validates the X-Internal-Key header against the configured key.
///
/// - Health check (`/api/health_check`) is always allowed through.
/// - If no `InternalKey` extension is present, all requests are allowed (dev mode).
/// - Otherwise, the `X-Internal-Key` header must match the configured value.
pub async fn validate_internal_key(
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Always allow health checks
    if request.uri().path() == "/api/health_check" {
        return Ok(next.run(request).await);
    }

    // If no key is configured, allow all requests (dev mode)
    let expected = match request.extensions().get::<InternalKey>() {
        Some(key) => key.0.clone(),
        None => return Ok(next.run(request).await),
    };

    // Validate the header
    let provided = request
        .headers()
        .get("x-internal-key")
        .and_then(|v| v.to_str().ok());

    match provided {
        Some(key) if key == expected => Ok(next.run(request).await),
        _ => {
            log::warn!(
                "Rejected request to {} - missing or invalid X-Internal-Key",
                request.uri().path()
            );
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}
