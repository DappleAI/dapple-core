//! Admin REST handlers for `rev.companies`.
//!
//! All four endpoints are admin-gated via `handler::admin::require_admin`.
//! The internal API surface is single-tenant, so we do NOT filter by
//! `caller.company_id` anywhere — see `crates/dapple-prompt-api/AGENTS.md`
//! "Single-Tenant Posture".

use crate::handler::admin::{AdminRejection, require_admin};
use crate::schema::companies::{
    CompanyDetailView, CompanyErrorResponse, CompanyView, CreateCompanyResponse,
    CreateCompanySchema, GetCompanyResponse, ListCompaniesQuery, ListCompaniesResponse,
    RevokeCompanyResponse, RevokeCompanySchema,
};
use crate::schema::external_service_tokens::ExternalServiceTokenView;
use crate::{AppState, AuthContext};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dapple_data::model::companies::CompanyModel;
use dapple_data::model::external_service_tokens::ExternalServiceTokenWithRevocationModel;
use dapple_data::query::companies::{
    insert_company, revoke_company, select_companies, select_company_by_id,
};
use dapple_data::query::external_service_tokens::select_external_service_tokens_by_company_id;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

/// Format an OffsetDateTime as ISO-8601.
fn format_time(dt: OffsetDateTime) -> String {
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| dt.to_string())
}

fn company_to_view(c: &CompanyModel) -> CompanyView {
    CompanyView {
        id: c.id,
        slug: c.slug.clone(),
        name: c.name.clone(),
        created_at: format_time(c.created_at),
        revoked_at: c.revoked_at.map(format_time),
    }
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

/// Map a sqlx error message into the right HTTP status. Slug/service_id
/// format violations and duplicate keys land here.
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

/// `POST /api/companies/` — create a B2B partner.
pub async fn create_company_handler(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Json(req): Json<CreateCompanySchema>,
) -> Result<impl IntoApiResponse, AdminRejection> {
    require_admin(&auth)?;

    // Trim user input. Slug format validation defers to the DB CHECK
    // constraint; surfacing the regex here would duplicate the source of
    // truth.
    let slug = req.slug.trim();
    let name = req.name.trim();

    if slug.is_empty() || name.is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(CompanyErrorResponse::new(
                "INVALID_INPUT",
                "slug and name must be non-empty",
            )),
        )
            .into_response());
    }
    if name.len() > 255 {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(CompanyErrorResponse::new(
                "INVALID_NAME",
                "company name must be 255 characters or fewer",
            )),
        )
            .into_response());
    }

    match insert_company(&state.db, slug, name).await {
        Ok(company) => Ok((
            StatusCode::CREATED,
            Json(CreateCompanyResponse {
                status: "success".to_string(),
                id: company.id,
                slug: company.slug,
                name: company.name,
                created_at: format_time(company.created_at),
            }),
        )
            .into_response()),
        Err(e) => {
            let (status, error, message) = map_data_error(e);
            Ok((status, Json(CompanyErrorResponse::new(error, message))).into_response())
        }
    }
}

/// `GET /api/companies/` — list partners (admin view).
pub async fn list_companies_handler(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Query(query): Query<ListCompaniesQuery>,
) -> Result<impl IntoApiResponse, AdminRejection> {
    require_admin(&auth)?;

    let include_revoked = query.revoked.unwrap_or(false);

    match select_companies(&state.db, include_revoked).await {
        Ok(rows) => {
            let objs: Vec<CompanyView> = rows.iter().map(company_to_view).collect();
            Ok((
                StatusCode::OK,
                Json(ListCompaniesResponse {
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

/// `GET /api/companies/{id}` — fetch one (plus its credentials).
pub async fn get_company_handler(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<impl IntoApiResponse, AdminRejection> {
    require_admin(&auth)?;

    let company = match select_company_by_id(&state.db, id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return Ok((
                StatusCode::NOT_FOUND,
                Json(CompanyErrorResponse::new("NOT_FOUND", "company not found")),
            )
                .into_response());
        }
        Err(e) => {
            let (status, error, message) = map_data_error(e);
            return Ok((status, Json(CompanyErrorResponse::new(error, message))).into_response());
        }
    };

    let tokens = match select_external_service_tokens_by_company_id(&state.db, id).await {
        Ok(rows) => rows.iter().map(token_to_view).collect(),
        Err(e) => {
            let (status, error, message) = map_data_error(e);
            return Ok((status, Json(CompanyErrorResponse::new(error, message))).into_response());
        }
    };

    let obj = CompanyDetailView {
        id: company.id,
        slug: company.slug,
        name: company.name,
        created_at: format_time(company.created_at),
        revoked_at: company.revoked_at.map(format_time),
        external_service_tokens: tokens,
    };

    Ok((
        StatusCode::OK,
        Json(GetCompanyResponse {
            status: "success".to_string(),
            obj,
        }),
    )
        .into_response())
}

/// `DELETE /api/companies/{id}` — mark revoked_at.
pub async fn revoke_company_handler(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    // `reason` is informational; the BAR pattern is mirrored from
    // DELETE /api/api-tokens but the DB column for company revocation
    // is just `revoked_at`. We accept and log the reason for parity.
    body: Option<Json<RevokeCompanySchema>>,
) -> Result<impl IntoApiResponse, AdminRejection> {
    require_admin(&auth)?;

    let reason = body.and_then(|b| b.reason.clone());
    if let Some(r) = &reason {
        log::info!("revoke_company {} (reason: {})", id, r);
    } else {
        log::info!("revoke_company {} (no reason)", id);
    }

    match revoke_company(&state.db, id).await {
        Ok(Some(_)) => Ok((
            StatusCode::OK,
            Json(RevokeCompanyResponse {
                status: "success".to_string(),
                message: "company has been revoked".to_string(),
            }),
        )
            .into_response()),
        Ok(None) => Ok((
            StatusCode::NOT_FOUND,
            Json(CompanyErrorResponse::new("NOT_FOUND", "company not found")),
        )
            .into_response()),
        Err(e) => {
            let (status, error, message) = map_data_error(e);
            Ok((status, Json(CompanyErrorResponse::new(error, message))).into_response())
        }
    }
}
