//! Client for the `/api/dispatch-phases/` endpoints.
//!
//! Dispatch phases are a read-only lookup table. Only the `All` query
//! variant is supported.

use crate::ClientCore;
use crate::SdkError;
use crate::types::DispatchStage;

/// Client for the `/api/dispatch-phases/` endpoints.
///
/// Note: The API uses "dispatch-phases" in the URL but the DTO is
/// named `DispatchStage` in the OpenAPI spec.
pub struct DispatchPhasesClient<'a> {
    client: &'a ClientCore,
}

impl<'a> DispatchPhasesClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// List all dispatch phases.
    pub async fn list(&self) -> Result<Vec<DispatchStage>, SdkError> {
        #[derive(serde::Serialize)]
        struct AllQuery {
            query: &'static str,
        }
        let query = AllQuery { query: "all" };
        self.client.get_many("api/dispatch-phases/", &query).await
    }
}
