//! External-surface value-slots client (`external` feature gated).
//!
//! Narrow read-only mirror of [`crate::resource::value_slots::ValueSlotsClient`]
//! containing only the methods applicable to the customer-facing surface
//! (`external-api.dapple.x`):
//!
//! - `get(id)` — `GET /api/value-slots/{id}` (single fetch, namespace-gated).
//! - `list_by_ids(ids)` — `GET /api/value-slots?query=by_object_ids&ids=...`
//!   (batch metadata fetch; foreign-namespace ids are silently dropped
//!   by the server so a third party guessing UUIDs gets no existence
//!   signal back). The `by_object_ids` variant name matches the internal
//!   surface's `GetValueSlotSetSchema::ByObjectIds`.
//! - `list(options)` — `GET /api/value-slots?page=&limit=` (paged listing
//!   scoped to the caller's company).
//!
//! Write-side operations (create/patch/delete) live on the internal
//! surface only — external consumers consume slot metadata, they don't
//! author slots. The path metadata accessors (`get_path`,
//! `list_paths_by_ids`) are also internal-only for the same reason.
//!
//! Wire-format note: this client's `list_by_ids` builds the query path
//! manually rather than delegating to [`crate::client::ClientCore::get_many`]
//! because the external `value_slots_index_handler` deserializes with
//! axum's standard `Query<T>` extractor, which requires repeated
//! `?ids=<uuid>&ids=<uuid>...` params. The default bracket-notation
//! serialization (`ids[0]=...&ids[1]=...`) used by `get_many` would not
//! deserialize into the handler's `Vec<Uuid>` field.

use crate::types::{FilterOptions, ValueSlot};
use crate::{ClientCore, SdkError};
use uuid::Uuid;

/// Customer-side value-slots client. Reached via
/// [`crate::ExternalDappleClient::value_slots`].
pub struct ExternalValueSlotsClient<'a> {
    client: &'a ClientCore,
}

impl<'a> ExternalValueSlotsClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// GET /api/value-slots/{id} — fetch a single value-slot in the
    /// caller's namespace. Returns 404 RESOURCE_NOT_FOUND for foreign
    /// or unknown UUIDs (identical body — no existence leak).
    pub async fn get(&self, id: Uuid) -> Result<ValueSlot, SdkError> {
        self.client.get_one(&format!("api/value-slots/{id}")).await
    }

    /// GET /api/value-slots?query=by_object_ids&ids=...&ids=... — batch
    /// fetch slot metadata by id. Foreign-namespace ids are silently
    /// dropped by the server (no error, just missing rows in the
    /// response), so callers can pass the union of slot UUIDs surfaced
    /// by a journey's brief-form without filtering first.
    ///
    /// Typical use: after `journeys().brief_form(id)`, collect every
    /// `value_slot_id` referenced by an `InputView`, dedup, and call
    /// `list_by_ids` once. Slot metadata (name, title, value_kind) is
    /// version-stable and cacheable by id.
    pub async fn list_by_ids(&self, ids: Vec<Uuid>) -> Result<Vec<ValueSlot>, SdkError> {
        #[derive(serde::Serialize)]
        struct Empty {}
        // Build the path manually with one ?ids=<uuid> per slot — see
        // module docstring for why we don't go through serde_qs here.
        let mut url =
            url::Url::parse("http://placeholder/api/value-slots").expect("static URL parses");
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("query", "by_object_ids");
            for id in &ids {
                q.append_pair("ids", &id.to_string());
            }
        }
        let path = match url.query() {
            Some(q) if !q.is_empty() => format!("api/value-slots?{q}"),
            _ => String::from("api/value-slots"),
        };
        self.client.get_many(&path, &Empty {}).await
    }

    /// GET /api/value-slots?page=&limit= — paged listing of every
    /// value-slot in the caller's namespace. Server filters by
    /// `owner_company_id = caller.company_id`.
    ///
    /// Flattens `FilterOptions` onto the wire as `?page=&limit=`, matching
    /// the rest of the external surface (`/api/guides`, `/api/playbooks`).
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<ValueSlot>, SdkError> {
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
        self.client.get_many("api/value-slots", &q).await
    }
}
