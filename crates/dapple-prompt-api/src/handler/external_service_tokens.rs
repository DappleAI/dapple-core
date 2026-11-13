//! Admin REST handlers for `rev.external_service_tokens`.
//!
//! All three endpoints are admin-gated via `handler::admin::require_admin`.
//! The Mig 37 trigger on `rev.external_service_tokens` AFTER INSERT
//! auto-creates the matching `rev.external_service_principals` row; this
//! handler never inserts into `external_service_principals` directly.

use crate::handler::admin::{AdminRejection, require_admin};
use crate::schema::companies::CompanyErrorResponse;
use crate::schema::external_service_tokens::{
    CreateExternalServiceTokenResponse, CreateExternalServiceTokenSchema, ExternalServiceTokenView,
    ListExternalServiceTokensResponse, RevokeExternalServiceTokenResponse,
    RevokeExternalServiceTokenSchema,
};
use crate::{AppState, AuthContext, parse_ed25519_public_key};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::model::external_service_tokens::ExternalServiceTokenWithRevocationModel;
use dapple_data::query::companies::select_company_by_id;
use dapple_data::query::external_service_tokens::{
    insert_external_service_token, revoke_external_service_token,
    select_external_service_token_by_id, select_external_service_tokens_by_company_id,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

fn format_time(dt: OffsetDateTime) -> String {
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| dt.to_string())
}

fn token_to_view(t: &ExternalServiceTokenWithRevocationModel) -> ExternalServiceTokenView {
    ExternalServiceTokenView {
        id: t.id,
        service_id: t.service_id.clone(),
        name: t.name.clone(),
        principal_id: t.principal_id,
        created_at: format_time(t.created_at),
        revoked_at: t.revoked_at.map(format_time),
    }
}

fn map_data_error(err: dapple_data::error::DataError) -> (StatusCode, String, String) {
    let msg = err.to_string();
    if msg.contains("duplicate key value violates unique constraint") {
        return (
            StatusCode::CONFLICT,
            "DUPLICATE".to_string(),
            "value already exists".to_string(),
        );
    }
    if msg.contains("violates check constraint") {
        return (
            StatusCode::BAD_REQUEST,
            "CHECK_CONSTRAINT".to_string(),
            "value violates database check constraint".to_string(),
        );
    }
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "DB_ERROR".to_string(),
        msg,
    )
}

/// `POST /api/companies/{id}/external-service-tokens/` — register a credential.
pub async fn create_external_service_token_handler(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Path(company_id): Path<Uuid>,
    Json(req): Json<CreateExternalServiceTokenSchema>,
) -> Result<impl IntoApiResponse, AdminRejection> {
    require_admin(&auth)?;

    let service_id = req.service_id.trim();
    let name = req.name.trim();
    let public_key_pem = req.public_key_pem.trim();
    let description = req
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if service_id.is_empty() || name.is_empty() || public_key_pem.is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(CompanyErrorResponse::new(
                "INVALID_INPUT",
                "service_id, name and public_key_pem must be non-empty",
            )),
        )
            .into_response());
    }

    // Validate the PEM ourselves before writing. The DB has no PEM-format
    // constraint; we'd rather reject at the boundary than silently store a
    // PEM the external auth flow can't load.
    if let Err(e) = parse_ed25519_public_key(public_key_pem) {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(CompanyErrorResponse::new(
                "INVALID_PEM",
                format!("invalid Ed25519 public key PEM: {}", e),
            )),
        )
            .into_response());
    }

    // Confirm the company exists. The FK violation would also catch this,
    // but the explicit lookup lets us return 404 instead of a generic
    // 5xx FK error envelope.
    match select_company_by_id(&state.db, company_id).await {
        Ok(Some(c)) => {
            if c.revoked_at.is_some() {
                return Ok((
                    StatusCode::CONFLICT,
                    Json(CompanyErrorResponse::new(
                        "COMPANY_REVOKED",
                        "company has been revoked; cannot issue new credentials",
                    )),
                )
                    .into_response());
            }
        }
        Ok(None) => {
            return Ok((
                StatusCode::NOT_FOUND,
                Json(CompanyErrorResponse::new(
                    "COMPANY_NOT_FOUND",
                    "company not found",
                )),
            )
                .into_response());
        }
        Err(e) => {
            let (status, error, message) = map_data_error(e);
            return Ok((status, Json(CompanyErrorResponse::new(error, message))).into_response());
        }
    };

    let inserted = match insert_external_service_token(
        &state.db,
        company_id,
        service_id,
        name,
        description,
        public_key_pem,
    )
    .await
    {
        Ok(row) => row,
        Err(e) => {
            let (status, error, message) = map_data_error(e);
            return Ok((status, Json(CompanyErrorResponse::new(error, message))).into_response());
        }
    };

    // The Mig 37 AFTER-INSERT trigger has already populated the
    // external_service_principals row. Look it up so we can return the
    // principal_id in the response.
    let principal_id = match select_external_service_token_by_id(&state.db, inserted.id).await {
        Ok(Some(row)) => row.principal_id,
        Ok(None) => {
            log::error!(
                "external_service_token {} created but lookup returned None — trigger may have failed",
                inserted.id
            );
            return Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(CompanyErrorResponse::new(
                    "PRINCIPAL_MISSING",
                    "external_service_principal row missing after insert",
                )),
            )
                .into_response());
        }
        Err(e) => {
            let (status, error, message) = map_data_error(e);
            return Ok((status, Json(CompanyErrorResponse::new(error, message))).into_response());
        }
    };

    // SHA-256 of the registered PEM lets the CLI confirm the exact bytes.
    let pem_sha256 = {
        let mut hasher = Sha256::new();
        hasher.update(public_key_pem.as_bytes());
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>()
    };

    Ok((
        StatusCode::CREATED,
        Json(CreateExternalServiceTokenResponse {
            status: "success".to_string(),
            id: inserted.id,
            service_id: inserted.service_id,
            name: inserted.name,
            principal_id,
            public_key_pem_sha256: pem_sha256,
            created_at: format_time(inserted.created_at),
        }),
    )
        .into_response())
}

/// `GET /api/companies/{id}/external-service-tokens/` — list credentials.
pub async fn list_external_service_tokens_handler(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Path(company_id): Path<Uuid>,
) -> Result<impl IntoApiResponse, AdminRejection> {
    require_admin(&auth)?;

    // Confirm the company exists so we can return 404 rather than an empty
    // list when the path is wrong.
    match select_company_by_id(&state.db, company_id).await {
        Ok(Some(_)) => (),
        Ok(None) => {
            return Ok((
                StatusCode::NOT_FOUND,
                Json(CompanyErrorResponse::new(
                    "COMPANY_NOT_FOUND",
                    "company not found",
                )),
            )
                .into_response());
        }
        Err(e) => {
            let (status, error, message) = map_data_error(e);
            return Ok((status, Json(CompanyErrorResponse::new(error, message))).into_response());
        }
    }

    match select_external_service_tokens_by_company_id(&state.db, company_id).await {
        Ok(rows) => {
            let objs: Vec<ExternalServiceTokenView> = rows.iter().map(token_to_view).collect();
            Ok((
                StatusCode::OK,
                Json(ListExternalServiceTokensResponse {
                    status: "success".to_string(),
                    results: objs.len(),
                    objs,
                }),
            )
                .into_response())
        }
        Err(e) => {
            let (status, error, message) = map_data_error(e);
            Ok((status, Json(CompanyErrorResponse::new(error, message))).into_response())
        }
    }
}

/// `DELETE /api/external-service-tokens/{id}` — revoke one credential.
pub async fn revoke_external_service_token_handler(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    body: Option<Json<RevokeExternalServiceTokenSchema>>,
) -> Result<impl IntoApiResponse, AdminRejection> {
    require_admin(&auth)?;

    let reason = body.and_then(|b| b.reason.clone());

    match revoke_external_service_token(&state.db, id, reason).await {
        Ok(Some(_)) => Ok((
            StatusCode::OK,
            Json(RevokeExternalServiceTokenResponse {
                status: "success".to_string(),
                message: "external service token has been revoked".to_string(),
            }),
        )
            .into_response()),
        Ok(None) => Ok((
            StatusCode::NOT_FOUND,
            Json(CompanyErrorResponse::new(
                "NOT_FOUND",
                "external service token not found",
            )),
        )
            .into_response()),
        Err(e) => {
            let (status, error, message) = map_data_error(e);
            Ok((status, Json(CompanyErrorResponse::new(error, message))).into_response())
        }
    }
}
