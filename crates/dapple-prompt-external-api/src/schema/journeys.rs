//! Request/response schemas for `POST /api/journeys` on the external surface.
//!
//! Mirrors the internal `dapple-prompt-api::schema::journeys` shape so the
//! wire format is consistent between surfaces. Customer-side validation
//! (every referenced UUID must belong to the caller's namespace) happens in
//! the handler.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateExternalJourneySetSchema {
    pub objs: Vec<CreateExternalJourneySchema>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateExternalJourneySchema {
    pub name: String,
    pub playbook_id: Uuid,
    #[serde(default)]
    pub brief_pack_ids: Vec<Uuid>,
    #[serde(default)]
    pub inference_profile_ids: Vec<Uuid>,
    /// Journey-chain primitive (Migration 44). Must be a journey in the
    /// caller's `owner_company_id` namespace. Foreign or unknown UUIDs
    /// return the same 404 body (no existence leak across namespaces).
    #[serde(default)]
    pub predecessor_journey_id: Option<Uuid>,
    /// Stage-level fork point (Migration 74). Optional; requires
    /// `predecessor_journey_id`. Must be a `rev.playbook_stages.id` of the
    /// PREDECESSOR's playbook. When set, this journey inherits only the
    /// predecessor bindings produced by stages strictly BEFORE the named
    /// stage ("fork at stage N" = redo N); when omitted, chaining inherits
    /// the predecessor's whole binding pool.
    #[serde(default)]
    pub fork_stage_id: Option<Uuid>,
    /// The specific `rev.guides.id` version this journey is pinned to.
    /// REQUIRED: the caller supplies the exact guide object (from the
    /// canonical-pin-aware browse); the backend validates it exposes
    /// `playbook_id` and never resolves "latest"/by-path. For chained journeys
    /// it must equal the predecessor's `guide_id`.
    pub guide_id: Uuid,
}

/// PATCH /api/journeys/{id} body — rename from the caller's perspective by
/// appending a `rev.principal_journey_preferences` row. The underlying
/// journey row is never mutated.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct UpdateExternalJourneySchema {
    pub name: String,
}

/// Query parameters for `GET /api/journeys` on the external surface.
///
/// Pagination only — no `query=...` variants; the caller's company is
/// implicit (taken from the JWT). The wire shape matches the rest of the
/// external surface's list endpoints (`/api/guides`, `/api/playbooks`):
/// flat `?page=&limit=` query params, NOT nested `options[..]`.
#[derive(JsonSchema, Serialize, Deserialize, Debug, Default)]
pub struct ListExternalJourneysQuery {
    /// Page size (defaults to 50 if omitted).
    #[serde(default)]
    pub limit: Option<u32>,

    /// 1-indexed page number (defaults to 1 if omitted).
    #[serde(default)]
    pub page: Option<u32>,
}
