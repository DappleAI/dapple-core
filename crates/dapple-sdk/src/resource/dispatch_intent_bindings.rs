//! Client for the `/api/dispatch-intent-bindings/` endpoints.

use crate::ClientCore;
use crate::SdkError;
use crate::types::{
    AllQuery, CreateDispatchIntentBindingSchema, CreateDispatchIntentBindingSetSchema,
    DispatchIntentBinding, FilterOptions, GetDispatchIntentBindingSetSchema,
};
use uuid::Uuid;

/// Client for the `/api/dispatch-intent-bindings/` endpoints.
pub struct DispatchIntentBindingsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> DispatchIntentBindingsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// List all dispatch intent bindings.
    pub async fn list(
        &self,
        options: Option<FilterOptions>,
    ) -> Result<Vec<DispatchIntentBinding>, SdkError> {
        let query = AllQuery::new(options);
        self.client
            .get_many("api/dispatch-intent-bindings/", &query)
            .await
    }

    /// List dispatch intent bindings by dispatch object IDs.
    pub async fn list_by_dispatch_ids(
        &self,
        dispatch_ids: Vec<Uuid>,
    ) -> Result<Vec<DispatchIntentBinding>, SdkError> {
        let query = GetDispatchIntentBindingSetSchema::ByDispatchIds { dispatch_ids };
        self.client
            .get_many("api/dispatch-intent-bindings/", &query)
            .await
    }

    /// Chain-aware effective-binding lookup (Migration 44): returns one
    /// binding per `value_slot_id` resolved via the journey-chain walk
    /// back (journey-local first, then upstream). Brief-form pre-fill on
    /// dapple-ui-demo's chapter N reads these to inherit chapter 1's
    /// values.
    pub async fn list_effective_by_journey_id(
        &self,
        journey_id: Uuid,
    ) -> Result<Vec<DispatchIntentBinding>, SdkError> {
        let query = GetDispatchIntentBindingSetSchema::EffectiveByJourneyId { journey_id };
        self.client
            .get_many("api/dispatch-intent-bindings/", &query)
            .await
    }

    /// Create one or more dispatch intent bindings.
    pub async fn create(
        &self,
        objs: Vec<CreateDispatchIntentBindingSchema>,
    ) -> Result<Vec<DispatchIntentBinding>, SdkError> {
        let body = CreateDispatchIntentBindingSetSchema { objs };
        self.client
            .post_many("api/dispatch-intent-bindings/", &body)
            .await
    }
}
