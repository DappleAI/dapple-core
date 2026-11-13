//! Public DTOs for `GET /api/journeys/{id}/brief-form`.
//!
//! Structural form data for a journey: briefs (with their content_hashes for
//! client-side cache keys), brief units, and the renderable subtype blocks
//! (input/notice/markdown). Designed for third-party form renderers that
//! cache the structure via ETag and poll bindings separately.
//!
//! Slot metadata (name, title, value_kind, constraints) is intentionally NOT
//! denormalized here — consumers batch-fetch via
//! `GET /api/value-slots?query=by_object_ids&ids=...` using the
//! `value_slot_id`s surfaced on `InputView`. Keeping slot metadata behind
//! a second fetch preserves independent caching: slot DTOs are immutable
//! per versioned id and can be cached indefinitely, while brief-form
//! invalidates on brief content_hash changes.

use crate::ContentHash;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Top-level response for `GET /api/journeys/{id}/brief-form`.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct JourneyBriefFormResponse {
    pub journey_id: Uuid,
    pub briefs: Vec<BriefFormEntry>,
}

/// One brief in the journey's brief-form, in journey traversal order.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct BriefFormEntry {
    pub id: Uuid,
    pub name: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Content-addressable SHA-1 of the brief; contributes to the endpoint's
    /// strong ETag so clients can cache the form indefinitely.
    pub content_hash: ContentHash,
    pub units: Vec<BriefUnitView>,
}

/// One brief unit, in render order. Exactly one of `input` / `notice` /
/// `markdown` is populated; the other two are `None`. Unknown / unsupported
/// subtypes are emitted with all three sub-blocks `None`.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct BriefUnitView {
    pub id: Uuid,
    /// Semantic key for the unit (stable across brief versions).
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<InputView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<NoticeView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<MarkdownView>,
}

/// Renderable input definition. `value_slot_id` is the FK callers use to
/// batch-fetch slot metadata via
/// `/api/value-slots?query=by_object_ids&ids=...`.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct InputView {
    /// Control flavor: `text`, `textarea`, `select`, `radio`, `checkbox`, ...
    pub control_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder_text: Option<String>,
    /// FK to `rev.value_slots.id`. Inputs without a slot reference are
    /// rejected at ingest, so this is always populated in V1.
    pub value_slot_id: Uuid,
    /// Pre-bound choice list for `select` / `radio` / `checkbox` controls.
    /// Empty for free-form inputs.
    #[serde(default)]
    pub choices: Vec<ChoiceView>,
    /// Selection cardinality of the bound slot: `"single"` (one item) or
    /// `"multi"` (keep-N subset — a `checkbox` collects several; the binding is
    /// array-valued). `#[serde(default)]` keeps older payloads deserializable.
    #[serde(default = "default_single_cardinality")]
    pub cardinality: String,
    /// For a `multi` slot: min/max items to keep (NULL = no floor / unbounded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_selections: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_selections: Option<i32>,
    /// Dynamic candidate source: the `value_slot_id` of the computed slot whose
    /// bound value supplies this control's choices at runtime (the
    /// generate→curate→refine candidate set). `None` for a static control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_source_value_slot_id: Option<Uuid>,
    /// Display name of the candidate-source slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_source_name: Option<String>,
    /// Author marked this single-value input optional (`:::brief-input
    /// { optional: true }`). `true` ⇒ the client renders no required indicator
    /// and does not gate submit on it; a blank field still binds `""`. Absent in
    /// older payloads ⇒ `false` (required) — the default for every existing
    /// input. Always emitted (not skipped) so consumers read it unconditionally.
    #[serde(default)]
    pub optional: bool,
}

fn default_single_cardinality() -> String {
    "single".to_string()
}

/// Renderable notice. `info` / `warning` / `error` / `success` per
/// `rev.brief_unit_notice.level`.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct NoticeView {
    pub level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail_text: Option<String>,
}

/// Renderable markdown block.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct MarkdownView {
    pub text: String,
    /// True when this markdown is a *deliverable*: its `$slot` placeholders have
    /// already been interpolated server-side with the journey's bound values.
    /// The UI renders these in the post-completion result view (not the input
    /// form). Ordinary prose markdown is `false`.
    #[serde(default)]
    pub deliverable: bool,
}

/// One choice on a `select` / `radio` / `checkbox` input.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct ChoiceView {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}
