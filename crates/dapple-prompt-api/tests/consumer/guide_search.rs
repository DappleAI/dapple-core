//! Consumer for the /api/guide-search/ endpoint.
//!
//! Unlike standard CRUD endpoints, the search endpoint does not implement
//! the Endpoint trait -- it has a unique query-only interface with no POST.

use crate::consumer::error::ConsumerError;
use crate::consumer::test_auth_header;
use dapple_data::model::guide_content_embeddings::GuideSearchResult;
use dapple_prompt_api::IndexResponse;
use reqwest::{Client, Url};
use uuid::Uuid;

pub struct GuideSearchEndpoint {
    pub base_url: Url,
}

impl GuideSearchEndpoint {
    /// Build the search URL with query parameters.
    ///
    /// Uses `Url::query_pairs_mut()` to properly encode all parameters.
    fn build_search_url(
        &self,
        q: &str,
        tags: Option<&[&str]>,
        locale: Option<&str>,
        limit: Option<i32>,
        guide_ids: Option<&[Uuid]>,
        threshold: Option<f64>,
    ) -> Result<Url, ConsumerError> {
        let mut url = self
            .base_url
            .join("api/guide-search/")
            .map_err(|e| ConsumerError::Custom(format!("URL parse: {}", e)))?;

        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", q);

            if let Some(tags) = tags {
                for tag in tags {
                    pairs.append_pair("tags", tag);
                }
            }
            if let Some(locale) = locale {
                pairs.append_pair("locale", locale);
            }
            if let Some(limit) = limit {
                pairs.append_pair("limit", &limit.to_string());
            }
            if let Some(guide_ids) = guide_ids {
                for (i, gid) in guide_ids.iter().enumerate() {
                    pairs.append_pair(&format!("guide_ids[{}]", i), &gid.to_string());
                }
            }
            if let Some(threshold) = threshold {
                pairs.append_pair("threshold", &threshold.to_string());
            }
        }

        Ok(url)
    }

    /// Perform a semantic search via GET /api/guide-search/.
    ///
    /// Returns the parsed search results, or an error describing what went wrong.
    pub async fn search(
        &self,
        q: &str,
        tags: Option<&[&str]>,
        locale: Option<&str>,
        limit: Option<i32>,
        guide_ids: Option<&[Uuid]>,
        threshold: Option<f64>,
    ) -> Result<Vec<GuideSearchResult>, ConsumerError> {
        let url = self.build_search_url(q, tags, locale, limit, guide_ids, threshold)?;

        let resp = Client::new()
            .get(url.clone())
            .header("Authorization", test_auth_header())
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            return Err(ConsumerError::Custom(format!(
                "GET {} failed with status {}: {}",
                url, status, text
            )));
        }

        let body: IndexResponse<GuideSearchResult> = serde_json::from_str(&text)?;
        assert_eq!(body.status, "success", "search response status");
        Ok(body.objs)
    }

    /// Return the HTTP status code for an empty query (q=).
    /// Expects the server to return 400 Bad Request.
    pub async fn search_empty_query(&self) -> Result<u16, ConsumerError> {
        let mut url = self
            .base_url
            .join("api/guide-search/")
            .map_err(|e| ConsumerError::Custom(format!("URL parse: {}", e)))?;

        url.query_pairs_mut().append_pair("q", "");

        let resp = Client::new()
            .get(url)
            .header("Authorization", test_auth_header())
            .send()
            .await?;

        Ok(resp.status().as_u16())
    }

    /// Return the HTTP status code for a search request.
    /// Useful for verifying 503 when conduit is unavailable.
    pub async fn search_status(&self, q: &str) -> Result<u16, ConsumerError> {
        let url = self.build_search_url(q, None, None, None, None, None)?;

        let resp = Client::new()
            .get(url)
            .header("Authorization", test_auth_header())
            .send()
            .await?;

        Ok(resp.status().as_u16())
    }
}
