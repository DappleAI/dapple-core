# Brief Flow Improvements — project plan

> **Granularity note.** AD-1 here ("per-stage is the finest granularity") describes *brief
> partitioning grain*, not read scope. Dispatch granularity (per-step is fine, see
> `per-step-dispatches.md`) and brief grain are both **orthogonal** to read availability:
> bindings are read journey-wide and order-independent (see
> `crates/dapple-prompt-flow/AGENTS.md` → "Binding read availability"). Neither implies value
> scoping.

Working project plan for full end-to-end brief-driven journey execution: rich brief-unit UIs in Studio, journey-wide binding persistence, asset upload, and integrated prose display. Touches `dapple-prompt` (backend), `dapple-guides` (builder), and `dapple-suite` (Studio).

## Status legend

- [ ] not started
- [~] in progress
- [x] done
- [!] blocked / needs decision

## Context

Studio's `/run/:guide_slug` runner races through every step without pausing for input. Under the hood several pieces are missing or disconnected:

1. **Backend: no brief-fetch endpoint** for a dispatch. `GET /api/dispatches/{id}/value-slots` does not exist; dapple-ui falls back silently when it 404s.
2. **Backend: dispatch_runner doesn't read bindings.** Grep of `dapple-prompt-flow` shows zero references to `dispatch_intent_bindings`. Tasks only read `dispatch.intent` (JSONB) and prior dispatches' `context_data`. Bindings are written but never consumed.
3. **Parser: no `brief-markdown` directive.** `KNOWN_DIRECTIVES` in `dapple-parser/src/lib.rs:23` lists `brief-input`/`brief-notice`/`brief-output` but not `brief-markdown`, even though `rev.brief_unit_types` already seeds `markdown` and `rev.brief_unit_markdown` already exists.
4. **Guide builder: no markdown unit emission.** `design-briefs-task` and `build-chapter-content-task.nu` don't produce markdown units, and the `design-briefs/en.md` prompt doesn't ask for them. Prose lives in chapter-level `:::prose` blocks, disconnected from brief forms.
5. **Guide ingest: no `brief-markdown` materialization.** Even though the data layer has `InsertBriefUnitSubtypeArgs::Markdown` (see `dapple-data/src/query/brief_units.rs:68`), `guide_ingest.rs`'s `materialize_brief_system()` never constructs it.
6. **SDK / suite API client: no brief-fetch wrapper.**
7. **Studio runner: no brief rendering.** `runner.rs` hard-codes `brief_pack_ids: Vec::new()`, never fetches briefs, never shows a form.

**Good news from the research:** the entire backend storage chain for markdown/notice/input/output units is in place. The per-task brief partitioning is solvable via a CTE over `rev.guide_blocks` (every block carries `chapter_id`, and stages + brief-groups are both blocks, so we can join `stage_name → chapter_id → sibling brief-groups → briefs → units`).

## Target architecture

Each task script's `$in` merges three streams, deterministic order:

1. **`$in.bindings.<slot_name>`** — typed values from `rev.dispatch_intent_bindings` joined journey-wide. NEW stream. Namespaced so it cannot collide with task output keys.
2. **`$in.<key>`** — prior task `context_data` (already wired in `dispatch_runner.rs:340-375`) and current dispatch's `intent` JSONB.
3. Accumulated outputs from prior tasks in the current dispatch (already wired).

Studio fetches `/api/dispatches/{id}/briefs` which returns per-task brief trees (full unit lists with ordering + per-input binding state). For each active task it renders the brief's units in order: markdown as display prose, notice as callouts, inputs as form controls (with asset upload for `asset` value_kind), outputs as live-computed result placeholders. Chapter sidebar groups steps by chapter. JSON debug panel + journey export + step replay round out the Studio GUI.

## Architectural decisions

### AD-1: Per-task brief partitioning via guide_blocks chapter linkage
Single CTE walks `dispatch_task → playbook_stage → stage_path_details.name → guide_blocks (directive_type=stage, name match) → chapter_id → sibling guide_blocks (directive_type=brief-group) → brief_group_path_details.name → brief_groups → brief_group_briefs → briefs → brief_units → typed subtypes + value_slots`. Grain: per-stage (chapter can have multiple stages; they share its brief-groups). For the SCORE guide, chapter = stage is 1:1.

### AD-2: dispatch_runner reads journey-wide bindings under `$in.bindings.*` namespace
Extend `execute_dispatch` to query `select_bindings_by_journey(journey_id)`, extract typed values per binding's `value_kind`, merge into `accumulated_data` under the key `bindings` (a `serde_json::Map`). Tasks reference `$in.bindings.customer_demographics`, not `$in.customer_demographics`. No collision possible with task output.

### AD-3: Per-step dispatching; bindings queried journey-wide
Keep per-step dispatching so choice-flow `dispatch.intent` carryover still works. Bindings are written against the current dispatch_id but the backend query joins `rev.dispatches` on `journey_id` for all binding reads, so users fill each slot once per journey.

### AD-4: Full asset upload chain in this pass
Studio implements file picker + chunked draft/chunk/finalize upload to dapple-prompt-api, receives back `asset.id`, stores in binding as `asset_items[].asset_id`. Uses existing `POST /api/drafts/`, chunk endpoints, and `POST /api/drafts/{id}/finalize/`.

### AD-5: Prose is a brief unit (`:::brief-markdown`)
Chapter-level prose does NOT flow into brief rendering. Instead, the guide builder emits `:::brief-markdown { name: "x" }` blocks that get materialized as brief units with subtype `markdown`. Briefs reference markdown units the same way they reference input/notice/output units (via `units: [...]` array). Studio renders them in order as display-only content.

Rationale:
- Keeps the rendering model uniform ("render the brief's units")
- No separate chapter-markdown loader in Studio
- Storage chain already exists (`rev.brief_unit_markdown`, `InsertBriefUnitSubtypeArgs::Markdown`)
- Guide builder can author prose adjacent to the form it explains, inside the same brief

Cost: parser + ingest + builder prompt + builder script + SCORE guide regeneration.

### AD-6: Studio sidebar groups by chapter; JSON export + manual replay included
- Chapter-grouped sidebar with dividers, current chapter highlighted
- "Export Journey JSON" button dumps `DispatchState` + journey context + all bindings
- "Replay Step" affordance re-runs the current step without advancing (useful for tuning brief values)

### AD-7: No Fluent message resolution in Studio
Labels: `slot.title || slot.name`. Brief unit text comes from inline markdown/notice text fields (not Fluent). Fluent integration is a separate feature.

### AD-8: Choice flow uses `dispatch.intent` (existing path)
When a task's `context_data` has a choice-like array (`choices`/`items`/`topics`/`options`), Studio renders a tile picker. Selection becomes next dispatch's `intent = {"selection": <value>}`. `dispatch_runner` already seeds dispatch intent into `accumulated_data`.

### AD-9: Collision-proof binding namespace
Bindings land under `$in.bindings.<slot_name>`. Task output / prior context stays under `$in.<key>`. No collision handling needed. Task scripts must be written to expect this namespacing.

## Work breakdown

### Phase 1 — Parser: recognize `:::brief-markdown` ✅

- [x] **1.1** Add `"brief-markdown"` to `KNOWN_DIRECTIVES` in `crates/dapple-parser/src/lib.rs`
- [x] **1.2** New transform module `crates/dapple-parser/src/transform/brief_markdown.rs` — extracts `BriefMarkdownDef { name, markdown_text, source_block_index }` from inline directive body
- [x] **1.3** New `BriefMarkdownDef` struct in `crates/dapple-parser/src/transform/mod.rs`
- [x] **1.4** `IngestPlan.derived_brief_markdowns: Vec<BriefMarkdownDef>` added; `extract_derived_from_block` signature + `brief_markdown` match arm + recursion call updated
- [x] **1.5** Four unit tests in `brief_markdown.rs` (all passing); 150/150 parser tests pass

### Phase 2 — Guide ingest: materialize markdown units ✅

- [x] **2.1** `markdown_map` added alongside input/notice/output maps in `materialize_brief_system()`
- [x] **2.2** `InsertBriefUnitSubtypeArgs::Markdown(...)` branch added to the unit-resolution loop with markdown_text from the parsed block
- [x] **2.3** `check_unique_names("brief-markdown", ...)` added to `validate_ingest_plan`
- [x] **2.4** `cargo build -p dapple-prompt-api` clean; also patched `shared_ingest.rs` IngestPlan constructor for the new field

### Phase 3 — Backend query: per-task briefs endpoint ✅

- [x] **3.1** Confirmed schema: `value_slot_path_details` has `(path_id, name, value_kind_id)`; `value_slots.title` lives on main table; `brief_unit_input` (singular); `brief_outputs` is a sibling table (not a subtype of brief_units); added `brief_markdown` to directive_types seed via migration 25
- [x] **3.2–3.7** Three raw `sqlx::query_as` functions added to `crates/dapple-data/src/query/briefs.rs`: `select_dispatch_task_briefs` (chapter linkage via stage name → guide_blocks), `select_brief_units_for_briefs` (LEFT JOINs against notice/markdown/input subtypes + input slot resolution via `control_spec->>'value_slot_id'`), `select_brief_outputs_for_briefs`, and `select_bindings_by_journey_of_dispatch` (DISTINCT ON latest binding per slot, joined with typed item tables). Handler `dispatches_briefs_get_handler` in `crates/dapple-prompt-api/src/handler/dispatches.rs` folds all four into `DispatchBriefsResponse`. Route `GET /api/dispatches/{id}/briefs` registered. New DTOs `DispatchBriefsResponse`, `DispatchTaskBriefs`, `ResolvedBrief`, `ResolvedBriefUnit` (tagged enum with Markdown/Notice/Input/Output variants), `ResolvedValueSlot`, `BoundValue` in `schema/dispatches.rs`. Used raw query_as instead of sqlc compose — faster iteration since schema changes touched several tables. Build passes.

**Skipped old 3.2 plan checklist items**:

- [ ] ~~**3.2** New `.sqlc` file~~
    ```
    (task_oid, brief_oid, brief_name, brief_title, unit_oid, unit_ord,
     unit_type, unit_name, unit_markdown_text, unit_notice_level, unit_notice_title,
     unit_control_type, unit_slot_id, unit_slot_name, unit_slot_title, unit_slot_value_kind,
     output_slot_id, output_slot_name, output_slot_title, output_slot_value_kind,
     binding_value_kind, binding_text, binding_int, binding_number, binding_bool, binding_json, binding_asset_id)
    ```
    (LEFT JOINs against each subtype table + LEFT JOIN on `dispatch_intent_bindings` joined through `dispatches` on `journey_id`. Fold into a hierarchical Rust model.)
- [ ] **3.3** `cargo sqlc compose` + `cargo sqlx prepare --workspace`
- [ ] **3.4** Rust query fn `select_briefs_by_dispatch(pool, dispatch_id)` in `crates/dapple-data/src/query/briefs.rs` with hierarchical fold: `Vec<TaskBriefs>` where each contains `briefs: Vec<BriefWithUnits>` where each contains `units: Vec<ResolvedBriefUnit>` ordered by `unit_ord`
- [ ] **3.5** New DTOs in `crates/dapple-prompt-api/src/schema/dispatches.rs`:
    ```rust
    pub struct ResolvedBriefUnit {
        pub unit_id: Uuid,
        pub name: String,
        pub kind: BriefUnitKind,  // Markdown / Notice / Input / Output / Media
        pub body: BriefUnitBody,  // variant-specific fields
    }
    pub enum BriefUnitBody {
        Markdown { text: String },
        Notice { level: String, title: Option<String>, detail: Option<String> },
        Input { control_type: String, slot: ResolvedValueSlot, bound_value: Option<BoundValue> },
        Output { slot: ResolvedValueSlot },
        Media { /* placeholder */ },
    }
    pub struct ResolvedValueSlot {
        pub id: Uuid,
        pub name: String,
        pub title: Option<String>,
        pub value_kind: String,
    }
    pub enum BoundValue {
        Text(String), Int(i64), Number(String), Bool(bool),
        Json(serde_json::Value), Asset { asset_id: Uuid },
    }
    pub struct TaskBriefs {
        pub task_oid: Uuid,
        pub briefs: Vec<ResolvedBrief>,
    }
    pub struct ResolvedBrief {
        pub id: Uuid,
        pub name: String,
        pub title: Option<String>,
        pub units: Vec<ResolvedBriefUnit>,
    }
    pub struct DispatchBriefsResponse {
        pub status: String,
        pub briefs_by_task: Vec<TaskBriefs>,
    }
    ```
- [ ] **3.6** Handler `dispatches_briefs_get_handler` in `crates/dapple-prompt-api/src/handler/dispatches.rs`
- [ ] **3.7** Route: `GET /{id}/briefs` in `crates/dapple-prompt-api/src/route/dispatches.rs`

### Phase 4 — Backend runtime: journey-wide binding reader ✅

- [x] **4.1** Reused `select_bindings_by_journey_of_dispatch` from Phase 3 (in `query/briefs.rs`) rather than duplicating the query.
- [x] **4.2** `execute_dispatch` in `dispatch_runner.rs` seeds `$in.bindings.<slot_name>` as a nested JSON object in `accumulated_data`, right after the prior-contexts seed. Failures log warnings and proceed without binding seed so task execution isn't blocked. New helper `binding_row_to_json` extracts typed values per `value_kind`.
- [ ] **4.3** Sanity test deferred to Phase 12 end-to-end verification.

### Phase 5 — Backend: batch binding write wrapper ✅

- [x] **5.1** Verified: `POST /api/dispatch-intent-bindings/` already accepts `CreateDispatchIntentBindingSetSchema { objs: Vec<...> }` batch shape via the existing handler at `crates/dapple-prompt-api/src/handler/dispatch_intent_bindings.rs:42`.
- [x] **5.2** No work needed.

### Phase 6 — SDK & suite API client ✅

- [x] **6.1** `dapple-sdk::resource::dispatches::briefs(dispatch_id)` added, wraps `get_one` against `api/dispatches/{id}/briefs`.
- [x] **6.2** DTOs (`DispatchBriefsResponse`, `DispatchTaskBriefs`, `ResolvedBrief`, `ResolvedBriefUnit` (tagged enum), `ResolvedValueSlot`, `BoundValue`) added to `crates/dapple-types/src/model/dispatches.rs`. Backend handler refactored to re-export these from `schema/dispatches.rs` so server and client share the same types. Handler now uses standard `get_response_handler` envelope.
- [x] **6.3** `packages/api/src/endpoints/dispatches.rs`: `get_dispatch_briefs(dispatch_id)` wrapper added.
- [ ] **6.4** ~~Batch `create_bindings`~~ — existing `create_binding` was changed; the suite already has `create_bindings(Vec<CreateIntentBinding>)` behavior via `sdk()?.dispatch_intent_bindings().create(...)` — verify in Phase 10 during runner wiring.
- [x] **6.5** `packages/api/src/lib.rs`: re-exports added for all 6 new DTO types.
- [ ] **6.6** `brief_pack_ids` wiring deferred to Phase 10 (runner rewrite) where it's actually consumed.

### Phase 7 — Guide builder: emit markdown units ✅

- [x] **7.1** `design-briefs/en.md`: added `brief-markdown` example block, updated brief_units schema to include `markdown` type with a `text` field, required every brief to start with a markdown unit, updated the worked example to show two chapter-prefixed markdown units leading their briefs.
- [x] **7.2** `design-briefs-task.nu`: no changes needed — script is a pass-through for LLM output; new unit type flows through `brief_units` array naturally.
- [x] **7.3** `build-chapter-content-task.nu`: added `markdown` branch in the `brief_unit_blocks` emission loop, emitting `:::brief-markdown { name }\n<text>\n:::` from `u.text`. Parse-check clean (94 hints, 0 errors).

### Phase 8 — Studio: base components ✅

**Completed files:**
- `views/runner_state.rs` — `DispatchPhase`, `DispatchState`, `TaskState`, `fetch_dispatch_state()`, `extract_choices()`, `ChoiceItem`. Three sequential API calls (briefs endpoint degrades gracefully on older dispatches). Phase derivation matches plan rule: failed → AwaitingInput (unbound input slot) → Ok (all tasks ok) → Running.
- `components/brief_renderer.rs` — renders markdown / notice / input / output unit variants in order; per-slot form state; reuses existing `TextInput`, `TextArea`, `SelectInput` components; `build_bindings()` helper maps `form_data` → `Vec<CreateDispatchIntentBinding>` per value_kind using the correct `CreateBinding*Schema` types; `bound_value_to_json()` pre-populates from server bindings.
- `components/choice_form.rs` — tile picker over `ChoiceItem[]`, calls back with selected `serde_json::Value`.
- `components/step_panel.rs` — phase-switched panel (awaiting_input → BriefRenderer or ChoiceForm fallback, running → spinner + per-task status, ok → results + Next/Replay, failed → error + Retry).
- `components/dispatch_debug_panel.rs` — collapsible pretty-printed JSON of the full DispatchState.
- Added `PartialEq` derive to all new DTOs in `dapple-types::model::dispatches` (Dioxus components require PartialEq on props).

**Skipped original sub-checklists** — implementation followed the spirit rather than the letter of the original Phase 8 task list.

- [ ] **8.1** New module `packages/dapple-studio/src/views/runner/dispatch_state.rs`:
    - `DispatchPhase { Running, AwaitingInput, Ok, Failed }`
    - `DispatchState { dispatch_id, phase, tasks, briefs_by_task, latest_context_data }`
    - `fetch_dispatch_state(client, dispatch_id)` — parallel fetches of dispatch tree + task attempts + briefs endpoint, then phase derivation
    - Phase rules: any attempt `failed` → Failed; else any unbound Input unit → AwaitingInput; else all attempts terminal Ok → Ok; else Running
- [ ] **8.2** New `packages/dapple-studio/src/components/brief_renderer.rs`:
    - `BriefRenderer(brief: ResolvedBrief, on_submit: EventHandler<HashMap<String, Value>>)`
    - Iterates `brief.units` in order:
        - `Markdown` → pulldown-cmark or plain text block with `whitespace-pre-wrap`
        - `Notice` → `alert alert-info|warning|success` DaisyUI class per level
        - `Input` → form control by slot.value_kind:
            - `text` → TextArea
            - `int` → NumberInput (integer)
            - `number` → NumberInput (decimal)
            - `bool` → Checkbox
            - `json` → TextArea placeholder
            - `asset` → `<AssetUploader />` (Phase 9)
        - `Output` → placeholder "Will populate after task runs" OR live context_data projection if available
    - Form state: `HashMap<slot_name, Value>` populated from pre-filled bindings (if user already submitted some) + user edits
    - Submit button only enabled when all required inputs have values
- [ ] **8.3** New `packages/dapple-studio/src/components/choice_form.rs`:
    - `ChoiceForm(items: Vec<ChoiceItem>, on_submit: EventHandler<Value>)`
    - Tile picker (DaisyUI card grid)
    - `extract_choices(context: &Value) -> Vec<ChoiceItem>` helper that scans `choices`/`items`/`topics`/`options` in order
- [ ] **8.4** New `packages/dapple-studio/src/components/step_panel.rs`:
    - Phase-switched view: briefs (AwaitingInput) / spinner (Running) / results (Ok) / error (Failed)
    - AwaitingInput: render each task's briefs via `BriefRenderer`; if no unbound inputs but task context has choices, show `ChoiceForm` instead
    - Ok state: renders formatted context_data and a "Next Step" button
    - Failed: error detail + "Retry" button
- [ ] **8.5** New `packages/dapple-studio/src/components/dispatch_debug_panel.rs`:
    - Collapsible toggle with pretty-printed DispatchState JSON
- [ ] **8.6** Update `packages/dapple-studio/src/components/mod.rs` with new exports

### Phase 9 — Studio: asset uploader ⚠️ deferred

**Status:** The public API endpoints for chunked draft upload don't exist on `dapple-prompt-api` — only `GET /api/drafts/{id}/content` is exposed. The chunked pipeline (`drafts → draft_chunks → finalize → asset`) is used internally by the guide ingest path but not surfaced as POST endpoints.

Since the SCORE guide (our end-to-end test target) has no `value_kind = asset` slots, building out the full chain adds significant backend scope with nothing to test against. `BriefRenderer` renders a clear "Asset uploader for '{label}' not yet wired" placeholder when an asset slot is encountered.

**Follow-up work (not in this pass):**
- Backend: `POST /api/drafts/` (create), `POST /api/drafts/{id}/chunks/` (append chunk), `POST /api/drafts/{id}/finalize/` (materialize asset) — handlers + routes + request schemas
- dapple-sdk: `resource::drafts::{create, append_chunk, finalize}` methods
- dapple-suite api: `create_draft`, `append_draft_chunk`, `finalize_draft` wrappers
- dapple-studio: `components/asset_uploader.rs` with file picker + 256KB chunker + progress bar
- Wire into `BriefRenderer`'s asset branch

Track as a separate plan file once a guide with asset slots exists and the flow can be tested.

### Phase 10 — Studio: runner rewrite ✅

**Key insight during implementation:** the chapter-linkage query in `select_dispatch_task_briefs` walks `dispatch → stages → chapter_id → brief-groups` directly — **it does not depend on `journey_brief_packs`**. So Studio can keep passing `brief_pack_ids: Vec::new()` to `create_journey` and the backend still returns the correct brief hierarchy per task. This eliminated the entire "how does Studio discover brief_pack_ids from the guide" sub-problem.

**Completed:**
- `views/runner.rs` rewritten as a state-driven runner using `DispatchState` signals and the component toolkit from Phase 8.
- State-machine callbacks: `on_start_journey`, `on_submit_bindings` (POSTs bindings, bumps `polling_nonce` to force next poll tick), `on_submit_choice` (creates next-step dispatch with `intent = {"selection": value}`), `on_next_step`, `on_retry`, `on_replay_step`.
- Polling loop: `use_effect` keyed on `(dispatch_id, polling_nonce)`. Stops when phase reaches `Ok` / `Failed`. Nonce bump restarts polling after bindings post.
- Added suite API client method `create_bindings_from_schemas` that bypasses the legacy `CreateIntentBinding` round-trip, so `BriefRenderer`'s schemas pass through directly.
- "Export Journey" button logs DispatchState to tracing for now (native file picker is a follow-up).
- Sidebar is still flat stage/step — chapter grouping is a nice-to-have deferred since it doesn't block end-to-end testing.

**Remaining sub-items from original plan:**
- [ ] **10.2** `brief_pack_ids` lookup — **not needed** per the chapter-linkage discovery above.
- [ ] **10.3** Chapter-grouped sidebar — **deferred**, flat sidebar still works.
- [x] **10.4** Manual step replay — implemented via `on_replay_step`.
- [x] **10.5** Journey JSON export — implemented (logs via tracing; native file dialog is follow-up).

Build: full dapple-suite workspace compiles clean (warnings only).

- [ ] **10.1** Rewrite `packages/dapple-studio/src/views/runner.rs`:
    - Keep guide/playbook loading, "Start Journey" button
    - State signals: `dispatch_id`, `dispatch_state`, `current_step_idx`, `running`, `error_msg`
    - On start: look up `brief_pack_ids` from the guide (see 10.2), create journey with them, create first step's dispatch, spawn polling task
    - Polling task: loop with 3s sleep, call `fetch_dispatch_state`, update signal, exit on Ok/Failed
    - `on_submit_bindings(form_data)` → `build_bindings(dispatch_id, form_data, current_brief_units)`, `client.create_bindings(...)`, polling resumes
    - `on_submit_choice(value)` → advance step idx, create next dispatch with `intent: {"selection": value}`, replace dispatch_id, restart polling
    - `on_next_step` → advance step idx, create next dispatch with empty intent (bindings still journey-scoped), restart polling
    - `on_retry` → re-create current dispatch
    - `on_replay_step` → tear down current dispatch, create a fresh one for the same step idx (see 10.4)
    - Render: sidebar (chapter-grouped, 10.3) + `StepPanel` + `DispatchDebugPanel` + "Export Journey JSON" button + "Replay Step" button
- [ ] **10.2** Guide → `brief_pack_ids` lookup. Check what `api::Guide` exposes today; if not the pack ids, add a client call `get_brief_packs_for_guide(guide_id)` that queries `/api/brief-packs/?query=by_guide&guide_id=...`. If no such query shape exists on the backend, add it as Phase 3.8.
- [ ] **10.3** Chapter-grouped sidebar: walk guide chapters + stages, build `Vec<ChapterOutline { title, stages: Vec<StageEntry> }>`. Render dividers + active-chapter highlight.
- [ ] **10.4** Manual step replay: abort current polling, call `create_dispatch` with the same step ref, replace dispatch_id. The guard is "replay is allowed only when current phase is Ok or Failed".
- [ ] **10.5** Journey JSON export: serialize `DispatchState` + all bindings + current step idx to a pretty JSON string, download as file (`download` attribute on anchor with `data:` URL, or `rfd` save dialog for native).

### Phase 11 — SCORE guide regeneration

- [ ] **11.1** Sync the updated guide-builder into the platform so Studio picks up new scripts + prompts
- [ ] **11.2** Delete the existing SCORE guide artifacts (user did this earlier for previous iteration)
- [ ] **11.3** Run the guide-builder create flow end-to-end to regenerate the SCORE guide, now with `:::brief-markdown` blocks inside each brief
- [ ] **11.4** Verify the regenerated chapter markdown includes markdown units and they parse cleanly
- [ ] **11.5** Ingest the regenerated guide via the normal Studio sync path

### Phase 12 — End-to-end verification

- [ ] **12.1** Compose + prepare + build backend:
    ```
    nu -c 'cd crates/dapple-data; cargo sqlc compose --source assets/sqlc/ --target assets/.sql/'
    nu -c 'cargo sqlx prepare --workspace'
    nu -c 'cargo build -p dapple-data -p dapple-prompt-api -p dapple-prompt-flow -p dapple-parser'
    ```
- [ ] **12.2** Redeploy API + flow:
    ```
    nu assets/nu/k8s/deploy_dapple_prompt_pods.nu --svc api
    nu assets/nu/k8s/deploy_dapple_prompt_pods.nu --svc flow
    ```
- [ ] **12.3** Build dapple-suite:
    ```
    nu -c 'cd ../dapple-suite; cargo build -p dapple-studio -p api'
    ```
- [ ] **12.4** Curl `/api/dispatches/<known-id>/briefs` and verify the response contains `briefs_by_task` with markdown, notice, input, output units in order, and a `bound_value` on any input that already has a binding
- [ ] **12.5** Launch Studio, go to `/run/score-business-plan-preparation`, click Start Journey
- [ ] **12.6** Verify:
    - [ ] First step shows chapter 1's brief with markdown prose → notice callout → text areas for inputs → output placeholders, in authored order
    - [ ] Chapter sidebar groups steps by chapter, chapter 1 highlighted
    - [ ] Submit bindings → spinner → task runs → task output appears in Ok state
    - [ ] Click Next Step → chapter 2 brief appears with chapter 1 slots no longer prompted (bound)
    - [ ] If any task emits a choices array, tile picker appears and selection flows into next step's intent
    - [ ] "Replay Step" re-runs current step with a fresh dispatch
    - [ ] "Export Journey" downloads JSON with full state
    - [ ] Debug panel shows raw DispatchState with binding info per task
    - [ ] A task that reads `$in.bindings.customer_demographics` successfully sees the user's submitted text

### Phase 13 — Move plan to repo

- [ ] **13.1** Copy this file to `docs/plans/brief-flow-improvements.md`
- [ ] **13.2** Commit with a reference PR description

## Known non-goals for this pass

- No Fluent message resolution for labels / help / placeholders — text comes from inline unit fields (AD-7)
- No `media` brief-unit rendering (schema supports it, leave for later)
- No resume-from-disk for interrupted journeys (bindings persist server-side, so this unblocks later)
- No per-step (sub-stage) brief partitioning — per-stage is the finest granularity (AD-1)
- No changes to existing non-SCORE guides — they'll continue to work without markdown units (briefs with only input/notice/output units render fine)

## File inventory

**dapple-prompt (backend):**
- `crates/dapple-parser/src/lib.rs` — add `brief-markdown` to KNOWN_DIRECTIVES
- `crates/dapple-parser/src/transform/brief_markdown.rs` (new)
- `crates/dapple-parser/src/transform/mod.rs` — BriefMarkdownDef + IngestPlan field + dispatch
- `crates/dapple-prompt-api/src/handler/guide_ingest.rs` — materialize markdown units + validation dedup
- `crates/dapple-data/assets/sqlc/briefs/select_briefs_by_dispatch.sqlc` (new)
- `crates/dapple-data/src/query/briefs.rs` — new `select_briefs_by_dispatch`
- `crates/dapple-data/src/query/dispatch_intent_bindings.rs` — new `select_bindings_by_journey`
- `crates/dapple-prompt-api/src/handler/dispatches.rs` — new `dispatches_briefs_get_handler`
- `crates/dapple-prompt-api/src/route/dispatches.rs` — new route
- `crates/dapple-prompt-api/src/schema/dispatches.rs` — new DTOs
- `crates/dapple-prompt-flow/src/dispatch_runner.rs` — binding-reader seed under `$in.bindings.*`
- `crates/dapple-sdk/src/resource/dispatches.rs` — new `briefs()` method
- `crates/dapple-sdk/src/types/dispatches.rs` — DTO definitions

**dapple-guides (builder):**
- `internal/dapple/guide-builder/prompts/design-briefs/en.md` — add `markdown` unit type
- `internal/dapple/guide-builder/scripts/build-chapter-content-task.nu` — `brief-markdown` directive emission

**dapple-suite (Studio):**
- `packages/api/src/endpoints/dispatches.rs` — `get_dispatch_briefs`
- `packages/api/src/endpoints/bindings.rs` — batch `create_bindings`
- `packages/api/src/endpoints/drafts.rs` + `assets.rs` — verify/add `create_draft`, `create_draft_chunk`, `finalize_draft`
- `packages/api/src/lib.rs` — re-exports
- `packages/dapple-studio/src/views/runner/dispatch_state.rs` (new)
- `packages/dapple-studio/src/views/runner.rs` — full rewrite
- `packages/dapple-studio/src/components/brief_renderer.rs` (new)
- `packages/dapple-studio/src/components/choice_form.rs` (new)
- `packages/dapple-studio/src/components/step_panel.rs` (new)
- `packages/dapple-studio/src/components/dispatch_debug_panel.rs` (new)
- `packages/dapple-studio/src/components/asset_uploader.rs` (new)
- `packages/dapple-studio/src/components/mod.rs` — exports
- `packages/dapple-studio/Cargo.toml` — add `rfd` (optional for native file picker)

**docs:**
- `docs/plans/brief-flow-improvements.md` (this file, moved on exit)
