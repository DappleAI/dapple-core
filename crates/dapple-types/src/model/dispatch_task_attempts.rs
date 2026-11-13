use crate::{DataTransferObject, DateTime, JsonObject};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for dispatch task attempt
#[derive(Clone, Eq, PartialEq, Debug, Deserialize, Serialize, JsonSchema)]
pub struct DispatchTaskAttempt {
    pub id: Uuid,
    pub dispatch_step_id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    #[schemars(with = "JsonObject")]
    pub context_data: serde_json::Value,
    #[schemars(with = "JsonObject")]
    pub context_metadata: serde_json::Value,
    /// Populated read-only convenience: the latest `failed`/`incomplete`
    /// event's `context_data.error` value, lifted from
    /// `rev.dispatch_task_attempt_events`. `None` when the attempt is
    /// not in a failure state, or when the failure event was emitted
    /// without an `error` field. The full event history remains
    /// available via the events table for richer diagnostics.
    #[serde(default)]
    pub error_detail: Option<String>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl DataTransferObject for DispatchTaskAttempt {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

/// One computed `:::brief-output`'s production result for a task attempt, as
/// surfaced to the external surface via [`ExternalDispatchTaskAttempt`].
///
/// The runtime producer (`dapple-prompt-flow::dispatch_runner`) records one of
/// these per computed output the producing step exposes, so a customer can see
/// *why* a computed slot is (or is not) bound without reading dapple pod logs.
/// `outcome` is one of:
/// - `produced` — the value was extracted and a binding was written;
/// - `produced_empty` — the value was extracted and a binding was written, but
///   the value was PRESENT AND EMPTY (an empty array/object, null, a blank
///   string, or a `multi` output that routed zero items). The slot is bound, so
///   this is not a failure; it is the distinction that lets a consumer tell
///   "produced nothing" from "produced something". A pick whose candidate
///   source lands here can never be satisfied, which is what the journey gates
///   read `rev.dispatch_intent_bindings.produced_empty` to determine;
/// - `source_path_missing` — the output's `source_path` did not resolve in the
///   task payload (no binding written — the slot stays unbound);
/// - `type_mismatch` — the resolved value could not be coerced to the slot's
///   `value_kind` (no binding written);
/// - `unsupported_kind` — the slot's `value_kind` is not producible.
#[derive(Clone, Eq, PartialEq, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ComputedOutputOutcome {
    pub value_slot_id: Uuid,
    pub name: String,
    pub outcome: String,
}

/// Operator-facing projection of [`DispatchTaskAttempt`] for the external API
/// surface.
///
/// The internal `context_metadata` is a control-plane grab-bag whose shape
/// depends on the event type — token metrics + author signals on task-outcome
/// events, but dapple-side directive identifiers on internal `flow_directive`
/// audit events. The customer surface must never expose those internals, so
/// this DTO drops `context_metadata` and surfaces only the author-emitted
/// `signals` sub-object (projected from `context_metadata.signals`; `null` for
/// internal-only events that carry no `signals` key). See the External-scope
/// authoring contract in `dapple-prompt-external-api/AGENTS.md`.
#[derive(Clone, Eq, PartialEq, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ExternalDispatchTaskAttempt {
    pub id: Uuid,
    pub dispatch_step_id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    #[schemars(with = "JsonObject")]
    pub context_data: serde_json::Value,
    /// Author-emitted task signals (`metadata.signals.*`) — the operator-facing
    /// talk-back channel (e.g. `menu_thin`, `items_deduped`, plus the token
    /// counts). Projected from the internal `context_metadata.signals`; `null`
    /// when the latest event carries no signals (e.g. an internal directive
    /// audit event).
    #[schemars(with = "JsonObject")]
    pub signals: serde_json::Value,
    /// Per-output results for the computed `:::brief-output`s this attempt's
    /// step produces. Projected from the runner-written
    /// `context_metadata.computed_outputs` (parallel to `signals`). Empty for
    /// attempts that produce no computed outputs. Lets a client tell apart "the
    /// server is still working" from "the producer ran but failed to produce
    /// this slot" (a `source_path_missing` / `type_mismatch` outcome) — the
    /// silent-stall failure mode that previously required dapple pod logs.
    #[serde(default)]
    pub computed_outputs: Vec<ComputedOutputOutcome>,
    /// Latest `failed`/`incomplete` event's `context_data.error`, or `None`.
    /// Same semantics as [`DispatchTaskAttempt::error_detail`].
    #[serde(default)]
    pub error_detail: Option<String>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
}

impl From<DispatchTaskAttempt> for ExternalDispatchTaskAttempt {
    fn from(a: DispatchTaskAttempt) -> Self {
        let signals = a
            .context_metadata
            .get("signals")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        // Defensive: a malformed/absent `computed_outputs` key yields an empty
        // Vec rather than a deserialize panic.
        let computed_outputs = a
            .context_metadata
            .get("computed_outputs")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        ExternalDispatchTaskAttempt {
            id: a.id,
            dispatch_step_id: a.dispatch_step_id,
            task_id: a.task_id,
            status: a.status,
            context_data: a.context_data,
            signals,
            computed_outputs,
            error_detail: a.error_detail,
            created_at: a.created_at,
        }
    }
}

impl DataTransferObject for ExternalDispatchTaskAttempt {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
