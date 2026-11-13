//! External-surface journeys client (`external` feature gated).
//!
//! Narrow mirror of [`crate::resource::journeys::JourneysClient`] containing
//! only the methods applicable to the customer-facing surface
//! (`external-api.dapple.x`):
//!
//! - `create(objs)` — `POST /api/journeys` (namespace-validates every
//!   referenced playbook / brief_pack / inference_profile UUID).
//! - `get(id)` — `GET /api/journeys/{id}` (single fetch, namespace-gated).
//! - `list(options)` — `GET /api/journeys` (paginated, scoped to the
//!   caller's company by the server).
//! - `update(id, name)` — `PATCH /api/journeys/{id}` (renames from the
//!   caller's perspective via `rev.principal_journey_preferences`).
//! - `hide(id)` — `DELETE /api/journeys/{id}` (soft-hides from the caller's
//!   own list views; idempotent).
//! - `briefs(id)` — `GET /api/journeys/{id}/briefs` (per-journey brief state).
//! - `stages(id)` — `GET /api/journeys/{id}/stages` (per-stage completion,
//!   overlay-driven).
//!
//! Internal-only operations (favorites, recent, list-by-user, list-by-playbook)
//! are absent by construction; if the external surface ever needs one,
//! it gets added here as a deliberate per-method decision, not by
//! inheritance.

use crate::types::{
    ChainSummary, CreateJourneySchema, CreateJourneySetSchema, FilterOptions, Journey,
    JourneyBindingsResponse, JourneyBrief, JourneyBriefFormResponse, JourneyStageStatus,
    JourneyStepStatus, UpdateJourneySchema,
};
use crate::{ClientCore, SdkError};
use uuid::Uuid;

/// Customer-side journeys client. Reached via
/// [`crate::ExternalDappleClient::journeys`].
pub struct ExternalJourneysClient<'a> {
    client: &'a ClientCore,
}

impl<'a> ExternalJourneysClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// POST /api/journeys — create one or more journeys in the caller's
    /// namespace. Each `playbook_id`, `brief_pack_id`, and
    /// `inference_profile_id` is namespace-validated server-side; foreign or
    /// unknown UUIDs return 404 RESOURCE_NOT_FOUND. The created journey
    /// carries `owner_company_id = caller.company_id` and `principal_id =
    /// caller.external_service_principal`.
    ///
    /// Wire-compatible with [`crate::resource::journeys::JourneysClient::create`]:
    /// the external surface accepts the same `CreateJourneySchema` shape (the
    /// extra fields like `brief_pack_ids`/`inference_profile_ids` are
    /// optional with `#[serde(default)]`).
    pub async fn create(&self, objs: Vec<CreateJourneySchema>) -> Result<Vec<Journey>, SdkError> {
        let body = CreateJourneySetSchema { objs };
        self.client.post_many("api/journeys", &body).await
    }

    /// GET /api/journeys/{id} — fetch a single journey in the caller's
    /// namespace. Returns 404 RESOURCE_NOT_FOUND for foreign or unknown
    /// UUIDs (identical body — no existence leak).
    pub async fn get(&self, id: Uuid) -> Result<Journey, SdkError> {
        self.client.get_one(&format!("api/journeys/{id}")).await
    }

    /// GET /api/journeys — list the caller's journeys, paginated via
    /// `FilterOptions` (`limit`, `page`). Server filters by
    /// `owner_company_id = caller.company_id`.
    ///
    /// Flattens `FilterOptions` onto the wire as `?page=&limit=`, matching
    /// the rest of the external surface (`/api/guides`, `/api/playbooks`).
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Journey>, SdkError> {
        #[derive(serde::Serialize, Default)]
        struct Query {
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<usize>,
            #[serde(skip_serializing_if = "Option::is_none")]
            page: Option<usize>,
        }
        let q = match options {
            Some(o) => Query {
                limit: o.limit,
                page: o.page,
            },
            None => Query::default(),
        };
        self.client.get_many("api/journeys", &q).await
    }

    /// PATCH /api/journeys/{id} — rename a journey from the caller's
    /// perspective. The customer's `external_service_principal` writes a
    /// `rev.principal_journey_preferences` row; the underlying journey row
    /// is untouched. Other principals (including the dapple internal
    /// surface) are unaffected.
    pub async fn update(&self, id: Uuid, name: impl Into<String>) -> Result<Journey, SdkError> {
        let body = UpdateJourneySchema { name: name.into() };
        self.client
            .patch_one(&format!("api/journeys/{id}"), &body)
            .await
    }

    /// DELETE /api/journeys/{id} — soft-hide the journey from the caller's
    /// own list views. Idempotent. Returns the resolved Journey with
    /// `hidden_at` populated.
    pub async fn hide(&self, id: Uuid) -> Result<Journey, SdkError> {
        self.client.delete_one(&format!("api/journeys/{id}")).await
    }

    /// GET /api/journeys/{id}/briefs — per-journey brief list, namespace-gated.
    /// Returns the resolved briefs hierarchy used by the runner UI; this is
    /// the journey-scoped counterpart to
    /// `dispatches().briefs(dispatch_id)` (which returns the per-dispatch
    /// view).
    pub async fn briefs(&self, id: Uuid) -> Result<Vec<JourneyBrief>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        self.client
            .get_many(&format!("api/journeys/{id}/briefs"), &Empty {})
            .await
    }

    /// GET /api/journeys/{id}/stages — per-stage completion for the journey.
    /// One entry per playbook stage with `done = true` once every value slot
    /// the stage *exposes* (its overlays) is bound somewhere in the journey
    /// chain; a stage that authored no overlays is trivially done. Entries are
    /// in authored (UUIDv7) stage order. Namespace-gated: foreign / unknown
    /// UUIDs return 404 RESOURCE_NOT_FOUND.
    ///
    /// This is the read side of the "completed _x_ stages" view; pair it with
    /// `brief_form(id)` (which only surfaces inputs for the exposed slots) and
    /// `dispatch_intent_bindings` to drive a per-stage progress UI.
    pub async fn stages(&self, id: Uuid) -> Result<Vec<JourneyStageStatus>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        self.client
            .get_many(&format!("api/journeys/{id}/stages"), &Empty {})
            .await
    }

    /// GET /api/journeys/{id}/steps — per-step exposed-slot status for the
    /// journey. Finer-grained than [`stages`](Self::stages): one entry per
    /// playbook step with `done = true` once every value slot the step exposes
    /// is bound in the journey chain, plus `exposed_slots` carrying each slot's
    /// `value_slot_id` (the bind target), `value_kind`, and `bound` flag.
    /// Namespace-gated: foreign / unknown UUIDs return 404 RESOURCE_NOT_FOUND.
    /// Use it to discover which step (e.g. a generate→pick→refine pick step) is
    /// awaiting a binding and which slot to set.
    pub async fn steps(&self, id: Uuid) -> Result<Vec<JourneyStepStatus>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        self.client
            .get_many(&format!("api/journeys/{id}/steps"), &Empty {})
            .await
    }

    /// GET /api/journeys/{id}/brief-form — structural form definition for
    /// the journey. Cacheable via the strong `ETag` header (raw SHA-1 over
    /// sorted brief content_hashes). This convenience method always fetches
    /// the full body; ETag-aware caching is left to the HTTP layer for now.
    ///
    /// Under value-slot overlays the form is **lockstep-filtered**: it surfaces
    /// only the inputs whose value slot is exposed by the journey's playbook
    /// stages (notice/markdown units and inputs without a slot are always
    /// kept). A guide that authored no overlays gets the full, unfiltered form.
    /// Foreign / unknown UUIDs return 404 RESOURCE_NOT_FOUND.
    pub async fn brief_form(&self, id: Uuid) -> Result<JourneyBriefFormResponse, SdkError> {
        self.client
            .get_flat(&format!("api/journeys/{id}/brief-form"))
            .await
    }

    /// GET /api/journeys/{id}/bindings — current binding state for the
    /// journey (latest binding per value_slot across all dispatches).
    /// Designed for cheap, frequent polling; the response carries
    /// `Cache-Control: no-store`. Foreign / unknown UUIDs return 404
    /// RESOURCE_NOT_FOUND.
    pub async fn bindings(&self, id: Uuid) -> Result<JourneyBindingsResponse, SdkError> {
        self.client
            .get_flat(&format!("api/journeys/{id}/bindings"))
            .await
    }

    /// GET /api/journeys/chains?guide_id=… — list chain summaries for
    /// the caller's company, scoped to one guide. UI groups by `root_id`
    /// to render one tile per chain (Migration 44 Phase 2).
    pub async fn list_chains(&self, guide_id: Uuid) -> Result<Vec<ChainSummary>, SdkError> {
        #[derive(serde::Serialize)]
        struct Q {
            guide_id: Uuid,
        }
        self.client
            .get_many("api/journeys/chains", &Q { guide_id })
            .await
    }

    /// GET /api/journeys/{id}/chain — chain (root → leaf) for one
    /// journey, namespace-gated. Foreign / unknown ids return 404 with
    /// identical bodies.
    pub async fn chain(&self, journey_id: Uuid) -> Result<Vec<Journey>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        self.client
            .get_many(&format!("api/journeys/{journey_id}/chain"), &Empty {})
            .await
    }
}
