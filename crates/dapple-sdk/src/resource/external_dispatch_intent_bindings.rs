//! External-surface dispatch-intent-bindings client (`external` feature gated).
//!
//! Single-method write client. Bindings are not externally listable — the
//! renderer reads binding state through the brief endpoints (per-journey or
//! per-dispatch). `POST` is the only customer-facing operation; the server
//! auto-spawns in-process execution for any dispatch that becomes fully
//! bound by the submission.

use crate::types::{
    CreateDispatchIntentBindingSchema, CreateDispatchIntentBindingSetSchema, DispatchIntentBinding,
};
use crate::{ClientCore, SdkError};

/// Customer-side dispatch-intent-bindings client. Reached via
/// [`crate::ExternalDappleClient::dispatch_intent_bindings`].
pub struct ExternalDispatchIntentBindingsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> ExternalDispatchIntentBindingsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// POST /api/dispatch-intent-bindings — submit operator inputs for
    /// value-slots on a dispatch. The server namespace-validates every
    /// `dispatch_id` and auto-spawns execution for any dispatch whose briefs
    /// are now fully bound.
    pub async fn create(
        &self,
        objs: Vec<CreateDispatchIntentBindingSchema>,
    ) -> Result<Vec<DispatchIntentBinding>, SdkError> {
        let body = CreateDispatchIntentBindingSetSchema { objs };
        self.client
            .post_many("api/dispatch-intent-bindings", &body)
            .await
    }
}
