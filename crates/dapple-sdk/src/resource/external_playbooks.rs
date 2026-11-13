//! External-surface playbooks client (`external` feature gated).
//!
//! Narrow customer-facing slice of
//! [`crate::resource::playbooks::PlaybooksClient`] — read-only:
//!
//! - `list(options)` — `GET /api/playbooks` (paginated; server filters by
//!   `owner_company_id = caller.company_id`).
//! - `get(id)` — `GET /api/playbooks/{id}` (single playbook with full
//!   nested stages/steps/tasks; namespace-gated; foreign or unknown UUID
//!   both return 404 RESOURCE_NOT_FOUND).
//!
//! Write operations (`create`, `delete`, `list_by_ids`, `list_by_slugs`) are
//! absent by construction. Customer playbooks are authored via the guide
//! ingest pipeline (`POST /api/guides/ingest`), not by direct creation
//! through this client.
//!
//! ### Wire-format note
//!
//! The server's `ListExternalPlaybooksQuery` accepts flat `limit` and `page`
//! query parameters — distinct from the journeys list shape (which nests
//! them under `options[..]`). The SDK takes [`FilterOptions`] for API
//! consistency and flattens it onto the wire.

use crate::types::{FilterOptions, Playbook};
use crate::{ClientCore, SdkError};
use uuid::Uuid;

/// Customer-side playbooks client. Reached via
/// [`crate::ExternalDappleClient::playbooks`].
pub struct ExternalPlaybooksClient<'a> {
    client: &'a ClientCore,
}

impl<'a> ExternalPlaybooksClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// GET /api/playbooks — list the caller's playbooks. Paginated via
    /// `FilterOptions` (`limit`, `page`). Server filters by
    /// `owner_company_id = caller.company_id`.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Playbook>, SdkError> {
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
        self.client.get_many("api/playbooks", &q).await
    }

    /// GET /api/playbooks/{id} — fetch a single playbook in the caller's
    /// namespace. Returns 404 RESOURCE_NOT_FOUND for foreign or unknown
    /// UUIDs (identical body — no existence leak).
    pub async fn get(&self, id: Uuid) -> Result<Playbook, SdkError> {
        self.client.get_one(&format!("api/playbooks/{id}")).await
    }
}
