use crate::model::briefs::BriefOutputDisplay;
use crate::model::journey_brief_form::ChoiceView;
use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Reference to a journey with minimal display data
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct JourneyRef {
    pub id: Uuid,
    pub name: String,
}

/// Per-stage completion status for a journey (overlay-driven).
///
/// `done` is true when every value slot the stage *exposes* (its overlays,
/// cascade-resolved stage→step→task) is bound somewhere in the journey chain.
/// A stage with no overlays exposes nothing and is trivially `done` (strict
/// default). Returned by `GET /api/journeys/{id}/stages` in authored
/// (UUIDv7) stage order.
///
/// `done` and `blocked` are not opposites and are never both true. A stage is
/// one of three things: `done`, *waiting* (neither flag — the operator still
/// owes something and can supply it), or `blocked` (see the field doc). Read
/// them in that precedence, because "not done" alone cannot tell a stage that
/// will finish from one that never can.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct JourneyStageStatus {
    pub playbook_stage_id: Uuid,
    /// Authored stage name (`rev.stage_path_details.name`) — the title a client
    /// renders for this pane. Absent in older payloads ⇒ empty string.
    #[serde(default)]
    pub name: String,
    /// Fluent-resolved stage description; the server falls back to the stage
    /// text, then the name, so it is always populated on current servers.
    /// Absent in older payloads ⇒ empty string.
    #[serde(default)]
    pub description: String,
    /// Authored closed-vocabulary stage kind (`:::stage { kind: "..." }`).
    /// Absent (unauthored, or an older payload) ⇒ the client applies its own
    /// documented default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<super::stages::StageKind>,
    /// Authored OPEN-vocabulary icon names (`:::stage { icon: [...] }`), in
    /// authored order, unvalidated and verbatim. A drawing client picks one and
    /// MUST degrade to its [`Self::kind`] glyph on a name it does not know; a
    /// non-drawing consumer ignores the field. Absent and `[]` are the same
    /// fact — empty is omitted from the JSON entirely. See
    /// [`super::stages::Stage::icon`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub icon: Vec<String>,
    pub done: bool,
    /// This stage can never reach `done` by any operator action: it holds a
    /// required pick whose candidate source produced a value that was present
    /// and EMPTY, so there is nothing to choose. Only re-running the producing
    /// task can clear it. Absent in older payloads ⇒ `false`.
    #[serde(default)]
    pub blocked: bool,
}

/// Render spec for an operator-bound exposed slot: the control flavor plus the
/// label/help/placeholder text and any pre-bound choices. Mirrors the
/// brief-form [`crate::model::journey_brief_form::InputView`] so a client can
/// render the control straight from `/steps` without a second join to
/// `/brief-form` on `value_slot_id`. `None` on a computed slot (nothing for an
/// operator to render — the server produces it).
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct ExposedSlotInput {
    /// Control flavor: `text`, `textarea`, `select`, `radio`, `checkbox`, ...
    pub control_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder_text: Option<String>,
    /// Pre-bound choice list for `select` / `radio` / `checkbox` controls.
    /// Empty for free-form inputs.
    #[serde(default)]
    pub choices: Vec<ChoiceView>,
    /// Dynamic candidate source: the `value_slot_id` of the computed slot whose
    /// bound value (an array of `{label, description, value}` tiles) supplies
    /// this control's choices at runtime. When set, the client reads the tiles
    /// from that slot's exposed-slot binding instead of `choices`. `None` for a
    /// statically-authored control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_source_value_slot_id: Option<Uuid>,
    /// Display name of the candidate-source slot (its path name), so a client
    /// can match it to the corresponding computed entry in `exposed_slots`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_source_name: Option<String>,
    /// Author marked this single-value input optional (`:::brief-input
    /// { optional: true }`). `true` ⇒ the client need not require a binding for
    /// it. Absent in older payloads ⇒ `false` (required) — the default. Mirrors
    /// [`crate::model::journey_brief_form::InputView::optional`]. Always emitted.
    #[serde(default)]
    pub optional: bool,
    /// Per-item field schema for a `repeater` control: a JSON array of resolved
    /// `{ key, control, label?, help?, placeholder? }` entries describing the
    /// editable fields of each list record. The client renders one form control
    /// per entry. `None` for every non-repeater control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_fields: Option<serde_json::Value>,
    /// Per-item refine LOCATOR slot for a `repeater` control: the slot the
    /// dedicated refine step exposes. The client finds the refine step in
    /// `/steps` by matching this id among a step's exposed slots, then runs an
    /// on-demand refine dispatch whose `{ record, notes }` ride in the dispatch
    /// intent (the slot is never bound; the refined record is read from the task
    /// attempt). `None` ⇒ the repeater offers no per-item refine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refine_input_value_slot_id: Option<Uuid>,
    /// Axis presentation config for a `timeline` control: a JSON object
    /// (`{ unit?, allowAbsolute? }`) the axis renderer reads (e.g. `unit:
    /// "minutes"` for a talk built over x minutes; `allowAbsolute` permits an
    /// entry to pin to an absolute axis position). `None` for non-timeline
    /// controls. A `list-ref` / `timeline-ref` field inside `item_fields` instead
    /// carries a `ref_value_slot_id` naming the separate accumulator slot it
    /// points at (the client's pane stack navigates to that slot's render spec).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<serde_json::Value>,
}

/// A value slot exposed by a step's overlays, with the version id a client
/// should bind, its kind (which `dispatch_intent_binding` item type to send),
/// whether it is already bound somewhere in the journey chain, and — the
/// discriminator clients should branch on — how it gets bound (`producer`).
///
/// `value_slot_id` is the JOURNEY-PINNED value_slot version (the same id the
/// journey's `/brief-form` exposes and bindings target), resolved from the
/// journey's pinned brief inputs / computed brief-outputs — NOT the
/// global-latest version at the path. It is populated for BOTH operator and
/// computed slots; it is `None` only in the genuine edge case where a path has
/// no pinned version at all. **Do not infer producer from `value_slot_id`** —
/// use the `producer` field.
///
/// `producer` is `"operator"` (the client POSTs a `dispatch_intent_binding`,
/// rendering the control from `input`) or `"computed"` (the server produces and
/// binds the value after the producing step's task succeeds — the client shows
/// a waiting/progress state and never tries to bind it). A `files`-sourced
/// output also reports `"operator"` (the operator uploads asset bindings).
///
/// `input` carries the render spec for `"operator"` slots; it is `None` for
/// `"computed"` slots.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct ExposedSlot {
    pub value_slot_id: Option<Uuid>,
    pub name: String,
    pub value_kind: String,
    pub bound: bool,
    /// This slot can never be bound by any operator action.
    ///
    /// Set on a required pick (`select` / `checkbox` / `repeater` / `timeline`)
    /// whose `candidate_source` slot is bound to a value that was present and
    /// EMPTY — the producing task succeeded and produced no choices, so there
    /// are no tiles to pick and nothing the operator can do. Only re-running
    /// the producer clears it.
    ///
    /// Never set together with `bound`, and never set on an optional or
    /// `min_selections = 0` pick, which is entitled to zero candidates. Treat
    /// `!bound && !blocked` as "waiting on the operator" — that is the
    /// distinction this field exists to make, since "not bound" alone cannot
    /// separate a slot that will be filled from one that never can be. Absent
    /// in older payloads ⇒ `false`.
    #[serde(default)]
    pub blocked: bool,
    /// `"operator"` | `"computed"` — see the type doc. `#[serde(default)]`
    /// keeps older payloads (pre-producer) deserializable; the data layer
    /// always populates it going forward.
    #[serde(default)]
    pub producer: String,
    /// Selection cardinality of the slot: `"single"` (exactly one item, the
    /// default) or `"multi"` (a keep-N subset — the binding is array-valued and
    /// the client renders a subset selector). `#[serde(default)]` keeps older
    /// payloads deserializable.
    #[serde(default = "default_single_cardinality")]
    pub cardinality: String,
    /// For a `multi` slot: minimum items to keep (None = no floor; `0` allows
    /// keeping none). Always None for `single`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_selections: Option<i32>,
    /// For a `multi` slot: maximum items to keep (None = unbounded). Always None
    /// for `single`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_selections: Option<i32>,
    /// Render spec for operator slots; `None` for computed slots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<ExposedSlotInput>,
    /// Render spec for computed slots (how to display the produced value);
    /// `None` for operator slots. Static authored hint only — the value itself
    /// stays on the bindings read, matched by `value_slot_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<ExposedSlotDisplay>,
}

/// Render spec for a computed exposed slot: how the client should display the
/// value once the server produces and binds it. The output-side analog of
/// [`ExposedSlotInput`] — operator slots carry `input`, computed slots carry
/// `display`. `None` on operator slots (which render via `input`). Carries only
/// the static, authored render hint (never the live value — that stays on the
/// bindings read, joined by `value_slot_id`).
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct ExposedSlotDisplay {
    /// How to render the bound value (image/text/markdown/list/checklist/table/
    /// fields), or `hidden` for a data-flow slot the client should not show.
    pub kind: BriefOutputDisplay,
    /// Heading label for the block, if authored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Collapse the block behind a "Reveal" spoiler toggle.
    #[serde(default)]
    pub reveal: bool,
    /// Sibling-format grouping: slots sharing a `group` are the SAME deliverable
    /// in different formats (e.g. a report as HTML and as PDF), and a client may
    /// render them as one block with a format switcher instead of unrelated
    /// sections. The token is **opaque** — any stable string the author picks;
    /// never parse it. `None` ⇒ never grouped, which is the pre-grouping
    /// behavior and remains the default.
    ///
    /// Two rules a renderer must honor:
    /// - **Filter `kind == Hidden` BEFORE grouping.** A hidden sibling is the
    ///   data-flow slot behind the deliverable (e.g. the source markdown the
    ///   rendered formats are built from); counting it produces a switcher with
    ///   a phantom entry.
    /// - **A group of one renders exactly like an ungrouped slot** — no switcher
    ///   chrome — so grouping a lone output is never a regression.
    ///
    /// Which sibling leads is not a field: slots arrive in authored order, so it
    /// is the first non-hidden member of the group, and the author changes it by
    /// moving the `:::brief-output` block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Short label naming this slot's format within its `group` (`"html"`,
    /// `"pdf"`, …) — what a client puts on the switcher control. A display
    /// label, NOT a mime type and NOT a closed vocabulary: an author may coin
    /// one (`"epub"`), so a client must render whatever arrives rather than
    /// matching against a known set. Localized copy belongs in `title`, which
    /// has a Fluent path; this is a format name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

fn default_single_cardinality() -> String {
    "single".to_string()
}

/// Per-step exposed-slot status for a journey (overlay-driven, finer than
/// [`JourneyStageStatus`]). `done` is true when every slot the step exposes is
/// bound in the journey chain; a step with no exposed slots is trivially
/// `done`. `exposed_slots` carries the pending (and already-bound) slots so a
/// client can discover which slot a generate→pick→refine step is waiting on and
/// bind it. Returned by `GET /api/journeys/{id}/steps` in authored (UUIDv7)
/// step order.
///
/// `done` and `blocked` are never both true. Read them in precedence —
/// `blocked`, then `done`, else waiting — because "not done" alone cannot tell
/// a step that will finish from one that never can.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct JourneyStepStatus {
    pub stage_step_id: Uuid,
    pub playbook_stage_id: Uuid,
    /// Authored name of the stage this step belongs to — repeated identically
    /// on every step row of a stage (consumers fold first-seen-wins). Absent in
    /// older payloads ⇒ empty string.
    #[serde(default)]
    pub stage_name: String,
    /// Fluent-resolved description of the owning stage; repeated per step like
    /// [`Self::stage_name`]. Absent in older payloads ⇒ empty string.
    #[serde(default)]
    pub stage_description: String,
    /// Authored closed-vocabulary kind of the owning stage; repeated per step.
    /// Absent (unauthored, or an older payload) ⇒ the client applies its own
    /// documented default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_kind: Option<super::stages::StageKind>,
    /// Authored OPEN-vocabulary icon names of the owning stage, in authored
    /// order; repeated identically on every step row of a stage like
    /// [`Self::stage_name`]. Unvalidated and verbatim — a drawing client
    /// degrades an unknown name to its [`Self::stage_kind`] glyph. Absent and
    /// `[]` are the same fact. See [`super::stages::Stage::icon`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stage_icon: Vec<String>,
    pub done: bool,
    /// Any slot this step exposes is [`ExposedSlot::blocked`] — the step can
    /// never reach `done` by an operator action. Absent in older payloads ⇒
    /// `false`.
    #[serde(default)]
    pub blocked: bool,
    pub exposed_slots: Vec<ExposedSlot>,
}

/// Reference to a brief pack attached to a journey
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq, Eq)]
pub struct BriefPackRef {
    pub id: Uuid,
}

/// Reference to an inference profile attached to a journey
#[derive(Clone, Debug, JsonSchema, Deserialize, Serialize, PartialEq, Eq)]
pub struct InferenceProfileRef {
    pub id: Uuid,
}

/// Public DTO for journey
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct Journey {
    pub id: Uuid,
    pub name: String,
    /// The actor that issued this journey — `rev.principals.id`. Replaces
    /// `user_id` as the identity surface; works for all four shapes.
    pub principal_id: Uuid,
    /// Legacy alias during the principals deprecation window. Carries the
    /// `rev.user_details.object_id` (Uuid) for human principals, `None` for
    /// non-human principals (external service flows). A future release drops
    /// this field; consumers should use `principal_id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<Uuid>,
    pub playbook_id: Uuid,
    pub brief_packs: Vec<BriefPackRef>,
    pub inference_profiles: Vec<InferenceProfileRef>,
    pub created_at: DateTime,
    /// Set when the *viewing* principal has soft-hidden this journey via
    /// DELETE /api/journeys/{id}. Per-principal: another principal viewing
    /// the same journey may see this as `None`. List endpoints filter out
    /// rows where this is set; single-get does not, so a freshly hidden
    /// journey can still be inspected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden_at: Option<DateTime>,
    /// Journey-chain primitive (Migration 44). When set, the binding-reader
    /// effective-lookup walks back through this link to inherit upstream
    /// chapters' values. `None` for chain roots and legacy ad-hoc journeys.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub predecessor_journey_id: Option<Uuid>,
    /// Journey-chain primitive (Migration 44). The specific `rev.guides.id`
    /// version this journey serves. `None` for journeys outside any guide
    /// context. When `predecessor_journey_id` is set, this must equal the
    /// predecessor's `guide_id` (enforced by DB trigger).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guide_id: Option<Uuid>,
    /// Stage-level fork point (Migration 74). When set (a
    /// `rev.playbook_stages.id` of the predecessor's playbook), the chain
    /// walk across this edge inherits only predecessor bindings produced by
    /// stages strictly BEFORE this stage ("fork at stage N" = redo N).
    /// `None` = plain whole-journey chain edge. Requires
    /// `predecessor_journey_id` (enforced by DB trigger).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_stage_id: Option<Uuid>,
}

/// Context information for a brief within a journey
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct JourneyBriefContext {
    pub brief_pack_id: Uuid,
    pub brief_group_id: Uuid,
    pub brief_group_name: String,
    pub brief_group_title: String,
}

/// Brief with journey context - public DTO for /journeys/{id}/briefs endpoint
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct JourneyBrief {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: ContentHash,
    pub title: String,
    pub description: Option<String>,
    #[schemars(with = "String")]
    pub created_at: DateTime,
    pub context: JourneyBriefContext,
}

impl DataTransferObject for Journey {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl DataTransferObject for JourneyBrief {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

// =============================================================================
// Phase 2: Chain enumeration DTOs
// =============================================================================
//
// `ChainSummary` is the unit returned by `GET /api/journeys/chains` — one
// entry per chain root, with the list of leaves (branches) under it. UI
// renders one tile per `ChainSummary`; a branch indicator appears when
// `branches.len() > 1` (sibling forks under the shared root).

/// One branch of a chain — a current leaf (no descendants) reachable
/// from the chain root.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct ChainBranch {
    pub leaf_id: Uuid,
    /// Depth of the leaf from the root (root = 0, immediate child = 1, …).
    pub depth: i32,
    pub leaf_playbook_id: Uuid,
    pub leaf_name: String,
    pub leaf_created_at: DateTime,
}

/// A chain summary — one chain root and its current branches. Returned
/// by the chains listing endpoint scoped to a `(principal, guide)` pair.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Deserialize, Serialize)]
pub struct ChainSummary {
    pub root_id: Uuid,
    pub root_name: String,
    pub root_created_at: DateTime,
    pub branches: Vec<ChainBranch>,
}

impl DataTransferObject for ChainSummary {
    fn id(&self) -> &Uuid {
        &self.root_id
    }
}
