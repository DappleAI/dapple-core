//! Admin gating helper.
//!
//! Both `handler::companies` and `handler::external_service_tokens` are
//! restricted to operators with the `"admin"` role. Since there is no
//! dedicated extractor for that (yet), each handler inspects the
//! `AuthContext` via this single helper. Returns an opaque `AdminRejection`
//! that serialises to HTTP 403; the helper does not write to logs.

use crate::{AuthContext, CallerKind};
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::Serialize;

/// 403 envelope returned by `require_admin`.
#[derive(JsonSchema, Serialize, Debug)]
pub struct AdminRejectionBody {
    pub error: String,
    pub message: String,
}

/// Opaque rejection type returned when admin gating fails.
#[derive(Debug)]
pub struct AdminRejection;

impl IntoResponse for AdminRejection {
    fn into_response(self) -> Response {
        (
            StatusCode::FORBIDDEN,
            Json(AdminRejectionBody {
                error: "FORBIDDEN".to_string(),
                message: "admin role required".to_string(),
            }),
        )
            .into_response()
    }
}

/// Returns `Ok(())` iff the caller has the `"admin"` role.
///
/// Recognises admin via the `roles` Vec on either `DelegatedUser` or
/// `ApiToken` caller kinds (both already carry the field — see lib.rs).
/// Service-only and external service-token callers are never admin in
/// this surface.
pub fn require_admin(auth: &AuthContext) -> Result<(), AdminRejection> {
    let roles: &[String] = match &auth.caller_kind {
        CallerKind::DelegatedUser(u) => &u.roles,
        CallerKind::ApiToken(t) => &t.roles,
        CallerKind::ServiceOnly | CallerKind::ServiceToken(_) => return Err(AdminRejection),
    };

    if roles.iter().any(|r| r == "admin") {
        Ok(())
    } else {
        Err(AdminRejection)
    }
}
