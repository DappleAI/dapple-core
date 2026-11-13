# Conduit Context Frames

**Status:** V1 shipped (2026-07-31) except where marked deferred.
**Theme:** unclouding the conduit's context — the model executing a guide's
prompts should see a deliberate, layered stack of purpose-built context, with
distractors actively removed. This is a **signal-quality** measure, not a
cost measure: the goal is that every token in the conduit's window helps it
reason about *this* guide, *this* pane, *this* task.

## The frame stack

Final system-message order for a dispatch-backed prompt:

```
[platform_frame, guide_frame, task_context, rag, ...template messages, user(context_data)]
```

| Scope | Frame | Delivery | Status |
|---|---|---|---|
| Platform | `# Platform Frame` — what a conduit is, how its output is used, how to read the other sections | Conduit-owned constant (`crates/dapple-prompt-conduit/assets/platform_frame.md`), injected on every dispatch-backed prompt | Shipped |
| Guide | `# Guide Frame` — the guide's CONDUITS.md, required per guide | Stored on `rev.guides.conduits_md` per version; a journey's pin resolves its exact frame; delivered via the task-context rail | Shipped |
| Journey | Structural session facts (fresh/chained/forked, fork stage, predecessor) | — | **Deferred** (see below) |
| Position | `## Position` in `# Dispatch Context` — pinned stage/step names + descriptions, "Stage N/M" | `select_task_position_for_dispatch_task` via `resolve_task_context` | Shipped |
| Prompt | The rendered template from `prompts/` | Existing | Shipped (pre-existing) |
| Aperture | Overlay blinders — expose/mask filters the conduit-visible bindings | Server-side filter in `resolve_task_context` keyed on `select_overlay_cascade_for_task` | Shipped |

Companion cleanup: RAG excerpt headers are neutral (`## Excerpt i/N`) —
SUMMARY.md/guide.toml titles are audience-voiced copy and never reach the
conduit; CONDUITS.md is the sole interpretive key for retrieved prose.

## Key decisions (with rationale)

- **CONDUITS.md is statically injected whole**, not RAG-retrieved: a frame
  that only appears when similarity search happens to fetch it is not a frame.
- **Hard requirement** (`ParseError::MissingConduits`): one parser check
  covers ingest POST/PATCH, dry-run, `dapple guide validate`, and external
  ingest. Empty file = warning.
- **Blinders reuse expose/mask as-is** — no new schema. The already-documented
  promise ("slots you don't expose aren't offered to the model") became true.
  Presence rule: zero overlays on the cascade ⇒ unfiltered (opt-in); any
  overlays ⇒ exposed subset only. Fails open (warn) — quality measure, not a
  security boundary.
- **Engine reads are never blinded**: `$in.bindings` stays journey-wide
  (flow "Binding read availability" invariant).
- **AGENTS.md / CONDUITS.md are siblings**, not derivations: AGENTS.md briefs
  the authoring agent, CONDUITS.md briefs the running model. Deriving a
  CONDUITS.md that can't state a binding crisply reveals an AGENTS.md gap —
  backfill it in the same pass.
- **V1 scope**: CONDUITS.md is English-only, root-level (like `scripts/`, not
  per-locale like `prompts/`).

## Deferred: the journey frame

A journey-scoped frame is worth revisiting (Kenny has a related idea cooking —
this section is its landing page). Analysis from the design discussion:

**Structural, value-free flavor (recommended if/when built):** deterministic
facts rendered from data we already hold — journey name; fresh vs chained vs
forked; the fork stage ("this journey redoes stage N of its predecessor —
earlier work inherited"); predecessor name; possibly stage-completion rollup.
Value-free so it cannot re-leak what blinders hide; no drift; a few lines on
the task-context rail. The genuinely missing signal today: a conduit refining
a fork cannot tell a redo from a fresh run.

**Narrative "story so far" flavor (rejected as anti-thesis):** either bakes
stale narrative (violating pin-exact-versions) or spends an LLM call per
dispatch generating context for another LLM — adding cloud, not removing it.
Frames should be row-exact and deterministic.

## Known unblinded channels (documented, deliberate)

- Whatever a task script pipes into `prompt send` (`context_data` → leftover
  user message) — author-owned. Possible future ingest lint: warn when a
  script interpolates `$in.bindings.<slot>` for a slot the step masks.
- `## Prior Task Outcomes` summaries (attempt `context_data` previews) are
  not slot-scoped in V1.

## Future refinements

- Per-guide CONDUITS.md size warning at plan time (prompt_responses snapshot
  growth; guidance is single-digit KB).
- Localized CONDUITS.md variants (per-locale like `prompts/`) if a non-English
  conduit surface ever exists.
- Journey frame (above), pending Kenny's follow-on design.
