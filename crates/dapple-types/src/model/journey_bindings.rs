//! Public DTOs for `GET /api/journeys/{id}/bindings`.
//!
//! Current binding state for a journey, deduplicated to the latest binding
//! per value_slot across all the journey's dispatches. `bound_at` drives
//! client-side staleness detection.

use crate::DateTime;
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Top-level response for `GET /api/journeys/{id}/bindings`.
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct JourneyBindingsResponse {
    pub journey_id: Uuid,
    pub bindings: Vec<BindingView>,
}

/// One binding row. `value_kind` selects which of the `*_value` / `asset_id`
/// fields is populated; all others are `None`.
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct BindingView {
    pub value_slot_id: Uuid,
    /// `text` | `int` | `number` | `bool` | `json` | `asset` — per
    /// `rev.value_kinds.key`.
    pub value_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub int_value: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub number_value: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bool_value: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_value: Option<serde_json::Value>,
    /// When `value_kind == "asset"`, the bound asset. Callers fetch the raw
    /// bytes via `GET /api/assets/{asset_id}/content`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<Uuid>,
    /// Full ordered array of ALL the slot's items, typed by `value_kind` (a
    /// `multi` slot binds one item per entry; a `single` slot has one). The
    /// scalar `*_value` fields above are only the FIRST item (a preview); this is
    /// the whole list. Consumers resolving an `options_slot` select dropdown from a
    /// bound slot read this. Always a JSON array (`[]` when unbound).
    #[serde(default)]
    pub item_values: serde_json::Value,
    /// When the binding was created. Latest binding per slot wins; this is
    /// that latest row's `created_at`.
    #[schemars(with = "String")]
    pub bound_at: DateTime,
}
