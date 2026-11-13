//! Client for brief units (accessed via parent brief).

use crate::types::BriefUnit;
use crate::{ClientCore, SdkError};
use uuid::Uuid;

pub struct BriefUnitsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> BriefUnitsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// List units for a specific brief.
    pub async fn list(&self, brief_id: Uuid) -> Result<Vec<BriefUnit>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        self.client
            .get_many(&format!("api/briefs/{brief_id}/units/"), &Empty {})
            .await
    }
}
