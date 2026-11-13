//! Request and response schemas for the admin `/api/companies/...` family.
//!
//! Top-level fields (no `obj` wrapper) on POST creates so the shape mirrors
//! `CreateApiTokenResponse` — see plan
//! `/.claude/plans/i-would-like-to-playful-boot.md`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::external_service_tokens::ExternalServiceTokenView;

/// `POST /api/companies/` body.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateCompanySchema {
    /// Slug (immutable; `^[a-z][a-z0-9_-]{1,31}$`).
    pub slug: String,
    /// Display name (free-form).
    pub name: String,
}

/// `POST /api/companies/` response. Top-level fields, no `obj` wrapper.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateCompanyResponse {
    pub status: String,
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub created_at: String,
}

/// Element used in the `GET /api/companies/` index response.
#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CompanyView {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

/// `GET /api/companies/` response.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct ListCompaniesResponse {
    pub status: String,
    pub results: usize,
    pub objs: Vec<CompanyView>,
}

/// `?revoked=true|false` filter on `GET /api/companies/`.
#[derive(JsonSchema, Serialize, Deserialize, Debug, Default)]
pub struct ListCompaniesQuery {
    /// When `Some(true)`, returns revoked rows too. Default `false`.
    pub revoked: Option<bool>,
}

/// `GET /api/companies/{id}` response payload.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CompanyDetailView {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
    pub external_service_tokens: Vec<ExternalServiceTokenView>,
}

/// `GET /api/companies/{id}` envelope. Uses the standard `obj` wrapper.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GetCompanyResponse {
    pub status: String,
    pub obj: CompanyDetailView,
}

/// `DELETE /api/companies/{id}` body.
#[derive(JsonSchema, Serialize, Deserialize, Debug, Default)]
pub struct RevokeCompanySchema {
    pub reason: Option<String>,
}

/// `DELETE /api/companies/{id}` response.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct RevokeCompanyResponse {
    pub status: String,
    pub message: String,
}

/// Error envelope used across the company / external-service-token endpoints.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CompanyErrorResponse {
    pub error: String,
    pub message: String,
}

impl CompanyErrorResponse {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }
}
