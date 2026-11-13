//! Models for the canonical org primitive: `rev.companies`.
//!
//! Companies are the namespace owners for path-bearing entities. The dapple
//! row uses a fixed UUID seeded by Migration 31; customer companies (e.g.
//! acme) are added by admins via the `/api/companies` endpoint family.
//!
//! See `docs/plans/external-api/00-architecture.md` §3 Decision 1.

use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

/// Internal model for `rev.companies`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CompanyModel {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub created_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
}
