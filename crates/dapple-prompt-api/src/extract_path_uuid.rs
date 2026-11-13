use aide::OperationInput;
use axum::Json;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use uuid::Uuid;

/// Custom UUID path extractor that returns structured JSON errors.
/// Wraps Axum's `Path<Uuid>` with API-consistent error responses.
pub struct ValidatedPathUuid(pub Uuid);

impl OperationInput for ValidatedPathUuid {}

pub struct PathUuidRejection {
    message: String,
}

impl IntoResponse for PathUuidRejection {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "INVALID_PATH_PARAM",
                "message": self.message,
            })),
        )
            .into_response()
    }
}

impl<S> axum::extract::FromRequestParts<S> for ValidatedPathUuid
where
    S: Send + Sync,
{
    type Rejection = PathUuidRejection;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        match Path::<Uuid>::from_request_parts(parts, state).await {
            Ok(Path(uuid)) => Ok(ValidatedPathUuid(uuid)),
            Err(rejection) => Err(PathUuidRejection {
                message: format!("Invalid UUID in path: {}", rejection.body_text()),
            }),
        }
    }
}
