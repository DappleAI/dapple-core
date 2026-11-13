//! Request/response schemas for the external-surface `GET /api/guides`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Query parameters for `GET /api/guides` on the external surface.
///
/// External customers do not interact with canonical release pins (those are
/// a dapple-team release-management concern), so there is no `environment`
/// parameter — the listing simply returns the latest non-deprecated guide per
/// path in the caller's namespace at scope=external.
///
/// `limit` and `page` mirror the internal API's `FilterOptions` shape.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct ListExternalGuidesQuery {
    /// Page size (defaults to 50 if omitted).
    #[serde(default)]
    pub limit: Option<u32>,

    /// 1-indexed page number (defaults to 1 if omitted).
    #[serde(default)]
    pub page: Option<u32>,
}
