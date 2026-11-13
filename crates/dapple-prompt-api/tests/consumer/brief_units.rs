//! Test consumer for brief units (read-only -- owned dependent of briefs).
//!
//! Brief units are created atomically with briefs and can only be read via
//! GET /api/briefs/{brief_id}/units/

use crate::consumer::error::ConsumerError;
use crate::consumer::test_auth_header;
use dapple_data::model::brief_units::BriefUnit;
use log::info;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Endpoint for brief units (nested under briefs, read-only)
pub struct BriefUnitEndpoint {
    pub base_url: Url,
    pub brief_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
struct IndexResponse {
    status: String,
    results: usize,
    objs: Vec<BriefUnit>,
}

impl BriefUnitEndpoint {
    /// URL for the units collection: /api/briefs/{brief_id}/units/
    pub fn units_url(&self) -> Result<Url, ConsumerError> {
        let url = format!("{}api/briefs/{}/units/", self.base_url, self.brief_id);
        Ok(Url::parse(&url)?)
    }

    /// GET all units for the brief
    pub async fn get_units(&self) -> Result<Vec<BriefUnit>, ConsumerError> {
        let url = self.units_url()?;

        let resp = Client::new()
            .get(url.clone())
            .header("Authorization", test_auth_header())
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;

        info!(
            "GET units resp: {}",
            serde_json::to_string_pretty(text.as_str())?
        );

        if status.as_u16() == 200 {
            let body: IndexResponse = serde_json::from_str(text.as_str())?;
            assert_eq!(body.status, "success", "GET units response status");
            Ok(body.objs)
        } else {
            panic!(
                "GET {} failed with status {}: {}",
                url,
                status.as_u16(),
                text
            );
        }
    }
}
