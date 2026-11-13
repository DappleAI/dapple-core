//! Request/response schemas for the external-surface `GET /api/playbooks`.
//!
//! Customers see only playbooks whose path lives in their company namespace.
//! There is no `query=...` variant — the listing is unambiguously
//! "the caller's playbooks", paginated.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Query parameters for `GET /api/playbooks` on the external surface.
///
/// `limit` and `page` mirror the internal API's `FilterOptions` shape.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct ListExternalPlaybooksQuery {
    /// Page size (defaults to 50 if omitted).
    #[serde(default)]
    pub limit: Option<u32>,

    /// 1-indexed page number (defaults to 1 if omitted).
    #[serde(default)]
    pub page: Option<u32>,
}
