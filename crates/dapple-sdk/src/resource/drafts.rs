use crate::ClientCore;
use crate::SdkError;
use uuid::Uuid;

/// Client for the `/api/drafts/` endpoints.
///
/// Drafts are in-progress content. Only the raw binary content endpoint
/// is exposed through the SDK.
pub struct DraftsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> DraftsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// Get the raw binary content of a draft.
    ///
    /// Note: draft content may be incomplete if the draft has not been finalized.
    pub async fn content(&self, id: Uuid) -> Result<bytes::Bytes, SdkError> {
        self.client
            .get_bytes(&format!("api/drafts/{id}/content"))
            .await
    }
}
