//! Client for the `/api/journeys/` endpoints.

use crate::types::{
    AddFavoriteJourneyChainResponse, AllQuery, ChainSummary, CheckFavoriteJourneyChainResponse,
    CreateJourneySchema, CreateJourneySetSchema, FilterOptions, GetJourneySetSchema, Journey,
    JourneyBindingsResponse, JourneyBrief, JourneyBriefFormResponse, JourneyStageStatus,
    JourneyStepStatus, ListFavoriteJourneyChainsResponse, RemoveFavoriteJourneyChainResponse,
    UpdateJourneySchema,
};
use crate::{ClientCore, SdkError};
use uuid::Uuid;

/// Client for journey operations.
pub struct JourneysClient<'a> {
    client: &'a ClientCore,
}

impl<'a> JourneysClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// Get a single journey by object ID.
    pub async fn get(&self, id: Uuid) -> Result<Journey, SdkError> {
        self.client.get_one(&format!("api/journeys/{id}")).await
    }

    /// List all journeys.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Journey>, SdkError> {
        let query = AllQuery::new(options);
        self.client.get_many("api/journeys/", &query).await
    }

    /// List journeys by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Journey>, SdkError> {
        let query = GetJourneySetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/journeys/", &query).await
    }

    /// List journeys by user object ID.
    pub async fn list_by_user(
        &self,
        user_id: Uuid,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Journey>, SdkError> {
        let query = GetJourneySetSchema::ByUser { user_id, options };
        self.client.get_many("api/journeys/", &query).await
    }

    /// List journeys by playbook object ID.
    pub async fn list_by_playbook(
        &self,
        playbook_id: Uuid,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Journey>, SdkError> {
        let query = GetJourneySetSchema::ByPlaybook {
            playbook_id,
            options,
        };
        self.client.get_many("api/journeys/", &query).await
    }

    /// Create one or more journeys.
    pub async fn create(&self, objs: Vec<CreateJourneySchema>) -> Result<Vec<Journey>, SdkError> {
        let body = CreateJourneySetSchema { objs };
        self.client.post_many("api/journeys/", &body).await
    }

    /// PATCH /api/journeys/{id} — rename the journey from the caller's
    /// perspective. The underlying journey is not mutated; this writes a
    /// `rev.principal_journey_preferences` row. Other principals' views are
    /// unaffected.
    pub async fn update(&self, id: Uuid, name: impl Into<String>) -> Result<Journey, SdkError> {
        let body = UpdateJourneySchema { name: name.into() };
        self.client
            .patch_one(&format!("api/journeys/{id}"), &body)
            .await
    }

    /// DELETE /api/journeys/{id} — soft-hide the journey from the caller's
    /// own list views. Idempotent. Other principals' views are unaffected.
    /// Returns the resolved Journey (with `hidden_at` populated).
    pub async fn hide(&self, id: Uuid) -> Result<Journey, SdkError> {
        self.client.delete_one(&format!("api/journeys/{id}")).await
    }

    /// Get briefs for a journey.
    pub async fn briefs(&self, journey_id: Uuid) -> Result<Vec<JourneyBrief>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        self.client
            .get_many(&format!("api/journeys/{journey_id}/briefs"), &Empty {})
            .await
    }

    /// GET /api/journeys/{id}/stages — per-stage completion for the journey.
    /// One entry per playbook stage with `done = true` once every value slot
    /// the stage *exposes* (its overlays) is bound somewhere in the journey
    /// chain; a stage that authored no overlays is trivially done. Entries are
    /// in authored (UUIDv7) stage order.
    ///
    /// This is the authoritative "completed _x_ stages" signal — prefer it over
    /// recomputing completion from `brief_form` + `bindings`. Note that
    /// `brief_form` is itself overlay-filtered (it surfaces only the inputs the
    /// exposed slots need), so the two stay in lockstep.
    pub async fn stages(&self, journey_id: Uuid) -> Result<Vec<JourneyStageStatus>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        self.client
            .get_many(&format!("api/journeys/{journey_id}/stages"), &Empty {})
            .await
    }

    /// GET /api/journeys/{id}/steps — per-step exposed-slot status for the
    /// journey. Finer-grained than [`stages`](Self::stages): one entry per
    /// playbook step with `done = true` once every value slot the step exposes
    /// is bound in the journey chain, plus `exposed_slots` carrying each slot's
    /// `value_slot_id` (the bind target), `value_kind`, and `bound` flag. Use it
    /// to discover which step (e.g. a generate→pick→refine pick step) is awaiting
    /// a binding and which slot to set. Entries are in authored (UUIDv7) order.
    pub async fn steps(&self, journey_id: Uuid) -> Result<Vec<JourneyStepStatus>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        self.client
            .get_many(&format!("api/journeys/{journey_id}/steps"), &Empty {})
            .await
    }

    /// GET /api/journeys/{id}/brief-form — structural form definition for
    /// the journey. The response carries an `ETag` header (raw SHA-1 over
    /// sorted brief content_hashes) clients can echo back via `If-None-Match`
    /// for a 304 short-circuit. This convenience method always fetches the
    /// full body; ETag handling is left to the HTTP layer for now.
    pub async fn brief_form(&self, journey_id: Uuid) -> Result<JourneyBriefFormResponse, SdkError> {
        self.client
            .get_flat(&format!("api/journeys/{journey_id}/brief-form"))
            .await
    }

    /// GET /api/journeys/{id}/bindings — current binding state for the
    /// journey (latest binding per value_slot across all dispatches).
    /// `bound_at` drives client-side staleness detection. Cheap to call;
    /// response carries `Cache-Control: no-store`.
    pub async fn bindings(&self, journey_id: Uuid) -> Result<JourneyBindingsResponse, SdkError> {
        self.client
            .get_flat(&format!("api/journeys/{journey_id}/bindings"))
            .await
    }

    /// List recent journeys.
    pub async fn recent(&self) -> Result<Vec<Journey>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        self.client.get_many("api/journeys/recent", &Empty {}).await
    }

    // ========================================================================
    // Favorites
    // ========================================================================

    /// Favorite the chain containing this journey. Server resolves the
    /// id to the chain root before storing.
    pub async fn favorite(&self, id: Uuid) -> Result<AddFavoriteJourneyChainResponse, SdkError> {
        self.client
            .post_one(&format!("api/journeys/{id}/favorite"), &())
            .await
    }

    /// Unfavorite the chain containing this journey. Idempotent.
    pub async fn unfavorite(
        &self,
        id: Uuid,
    ) -> Result<RemoveFavoriteJourneyChainResponse, SdkError> {
        self.client
            .delete_one(&format!("api/journeys/{id}/favorite"))
            .await
    }

    /// Check whether the chain containing this journey is favorited.
    pub async fn is_favorite(
        &self,
        id: Uuid,
    ) -> Result<CheckFavoriteJourneyChainResponse, SdkError> {
        self.client
            .get_one(&format!("api/journeys/{id}/favorite"))
            .await
    }

    /// List all favorite journey chains for the authenticated caller.
    pub async fn favorites(&self) -> Result<ListFavoriteJourneyChainsResponse, SdkError> {
        self.client.get_one("api/journeys/favorites").await
    }

    // ========================================================================
    // Phase 2: chain enumeration (Migration 44)
    // ========================================================================

    /// GET /api/journeys/chains?guide_id=… — list chain summaries for the
    /// authenticated caller, scoped to one guide. UI groups by `root_id`
    /// to render one tile per chain, with a branch indicator when
    /// `branches.len() > 1`.
    pub async fn list_chains(&self, guide_id: Uuid) -> Result<Vec<ChainSummary>, SdkError> {
        #[derive(serde::Serialize)]
        struct Q {
            guide_id: Uuid,
        }
        self.client
            .get_many("api/journeys/chains", &Q { guide_id })
            .await
    }

    /// GET /api/journeys/{id}/chain — chain (root → leaf) for a single
    /// journey. Used by the dapple-ui-demo resolver to drop the "latest
    /// journey per playbook" lookup. 404 on foreign or unknown ids.
    pub async fn chain(&self, journey_id: Uuid) -> Result<Vec<Journey>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        self.client
            .get_many(&format!("api/journeys/{journey_id}/chain"), &Empty {})
            .await
    }
}
