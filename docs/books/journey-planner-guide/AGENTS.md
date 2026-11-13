# Agent Reading Guide — Journey Planner Guide

## What this book is for

This book teaches you how to **author a Guide** — the directory of markdown chapters, `guide.toml` metadata, Fluent `.ftl` files, and assets that, when ingested, produces playbooks, briefs, tasks, inference profiles, and directives in the platform.

Read this if you are authoring or modifying a Guide directory. It does **not** explain how the API is consumed (see `api-consumer-guide/`) or how the parser/ingest pipeline is implemented internally (see `journey-builder-guide/`).

> **"Guide"** is a *platform entity*. The book you are reading is *about* Guides. They are not the same thing. The book about authoring Guides happens to live in `docs/books/journey-planner-guide/`. Confusing? Yes. Real? Yes.

## Read this first if…

| You're trying to… | Start here |
|--------------------|------------|
| Lay out a new Guide directory | `guide-directory-structure.md` |
| Configure metadata, cards, render profiles | `guide-toml-reference.md` |
| Write directive blocks (`:::stage`, `:::step`, `:::nushell`, `:::brief_pack`, etc.) | `directive-blocks.md` |
| Localize text into multiple locales | `fluent-localization.md` |
| Configure inference profiles (LLM model selection per task/locale) | `defining-inference-profiles.md` |
| Control flow with retry / loop / branch / skip | `directives.md` |
| Walk an end-to-end real example | `resume-guide-walkthrough.md` |
| Understand the ingest pipeline | `ingest-pipeline.md` |
| Reuse content via shared content packs | `shared-content.md` |
| Version a Guide for republish | `versioning.md` |
| Test or debug your authored guide | `testing-debugging.md` |

## Concepts you must already understand

These are not negotiable. The terse versions live in `docs/books/README.md` ("Start here, agent"); the canonical versions are in `CLAUDE.md` and `crates/dapple-parser/AGENTS.md`. Most relevant for guide authors:

- **Guides are Fluent-native.** The Guide structure is locale-independent. All localizable text — prose body, stage description, step description, brief-pack content, inference-profile system prompt — lives in `.ftl` files under `locales/`. Directive blocks for these types carry `content: None` after parsing; their text is resolved from a Fluent message keyed by the directive's `name` attribute. Inline blocks (`nushell`, `ref`) keep their content because code and structural references are not localized.
- **Message-key convention.** Fluent message keys are kebab-case and match the directive's `name` attribute. Attributes (`.system-prompt`, `.temperature`, `.card-subtitle`, etc.) attach metadata.
- **Ingest is content-addressable.** A SHA-1 hash is computed over canonical content; on re-ingest, a matching hash returns the existing version. Parents Merkle-hash sorted child hashes — change one task and its step, stage, playbook hashes propagate up.
- **Embedding generation is async.** Ingest returns before the conduit has written embeddings to `rev.guide_content_embeddings`. Search is eventually consistent with a fresh ingest.
- **Journey-centric vocabulary.** Authors say *Debrief* (for what users see), *Journey*, *Playbook*, *Dispatch*. The internal name "Brief" still appears in code and some docs; treat it as the back-end name for what the user sees as a Debrief.
- **`run` is one-shot.** A directive's `{ "run": [] }` primitive executes the wrapped stage/step at most once per evaluation, regardless of how many times you reference it in JSONLogic.
- **`assets/.sql/` is generated.** If you change SQL while authoring a guide that ships with platform code, edit `.sqlc` sources in `assets/sqlc/` and run `cargo sqlc compose`. Never edit `.sql/`.
- **Always run shells via `nu -c '...'`.** Bare bash has none of the platform's environment.

## Quick navigation

| Chapter | What it covers |
|---------|----------------|
| `intro.md`, `concepts.md` | Mental model, journey-centric philosophy |
| `guide-directory-structure.md` | Layout of a guide on disk |
| `guide-toml-reference.md` | Metadata, card config, classification, render profiles |
| `directive-blocks.md` | All directive types: `prose`, `stage`, `step`, `nushell`, `brief_pack`, `inference_profile`, `ref`, `block` |
| `directives.md` | JSONLogic, `run` primitive, action values, branching |
| `fluent-localization.md` | `.ftl` files, message keys, attributes, fallbacks |
| `defining-inference-profiles.md` | Per-task/locale model selection |
| `writing-nushell-tasks.md` | Nu task contract, `{ payload, signals }`, security policy |
| `ingest-pipeline.md` | tarball → parse → `plan_guide()` → materialize → async embedding |
| `shared-content.md` | `_shared/common`, `_shared/internal`, `:::ref` blocks |
| `versioning.md` | Path/version triplets, deprecation |
| `testing-debugging.md` | `dry_run`, parser test fixtures, common failure modes |
| `dapple-prompt-conduit.md` | LLM routing summary (read `journey-builder-guide/conduit/` for internals) |
| `dapple-prompt-flow.md` | Workflow runtime summary |
| `glossary.md` | Terminology |

## Common quick answers

### Author a directive block

```markdown
:::stage { name: "topic-selection" }
:::

:::step { name: "brainstorm-topics" }
:::
```

The text comes from `locales/en.ftl`:

```ftl
topic-selection =
    Guide the user from initial brainstorming through to a focused
    thesis statement.

brainstorm-topics =
    Generate a list of potential essay topics.
    .description = Extended brainstorm description
```

### Author a step's tasks (nest them — recommended)

A step is an **ordered set of tasks that run as one fail-together unit**; multiple tasks per step is the norm. Write the `:::nushell` tasks as **children inside the `:::step` fence** — they become the step's private tasks, auto-scoped to the step name and run in document order. (Data retrieval is just a task inside the step.)

```markdown
:::step { name: "Build the menu", expose: ["menu"] }

:::nushell { name: "get" }
{ payload: { ... }, signals: { } }
:::

:::nushell { name: "mangle" }
{ payload: { ... }, signals: { } }
:::

:::nushell { name: "check-safety" }
{ payload: { ... }, signals: { } }   # return signals.unsafe = true to veto
:::

:::
```

Tasks run in order; the first failure stops the step. A task can veto with `signals.unsafe = true` (terminal `unsafe`, non-retryable). A computed `:::brief-output` publishes only when the whole step succeeds, so keep its safety check in the same step, after the producer. Alternatively, reference shared tasks with `tasks: ["a","b"]` (or `task: "x"` sugar) — there is **no** `task_ref:` attribute. See [`concepts.md`](./concepts.md) → "Authoring a step's tasks" and [`directive-blocks.md`](./directive-blocks.md) → "Nesting Rules".

### Ingest the guide

```bash
nu -c 'curl -X POST "$env.DAPPLE_PROMPT_API_URL/api/guides/ingest?dry_run=false" \
    -H "Authorization: Bearer $env.DAPPLE_TOKEN" \
    -F "file=@my-guide.tar.gz"'
```

Or via SDK — see `api-consumer-guide/ingest-api.md`.

### Re-ingest after edits

If your changes don't affect content (whitespace, comments-only): the hash matches and the existing version is returned. To force a new version, change something semantically meaningful or use the PATCH endpoint with explicit version bump.

### Author a directive (flow control)

```json
{
  "if": [
    { "==": [{ "var": "run.status" }, "ok"] },
    "proceed",
    { "if": [
      { "in": [{ "var": "run.error_code" }, ["RATE_LIMITED", "TIMEOUT"]] },
      "retry",
      "fail"
    ]}
  ]
}
```

Set `max_attempts` and `retry_delay_ms` on the directive. Step directives may only branch within their stage; stage directives may branch across the playbook.

## Source-file references (when chapters cite code)

| Path | What lives there |
|------|------------------|
| `crates/dapple-parser/src/...` | Fence tokenizer, block classifier |
| `crates/dapple-parser/src/transform/...` | `plan_guide()`, `IngestPlan`, derived-object extraction |
| `crates/dapple-fluent/src/...` | Fluent message parsing and rendering |
| `crates/dapple-data/assets/sqlc/` | SQL **sources** — author here |
| `crates/dapple-data/assets/.sql/` | SQL **generated output** — never edit |
| `crates/dapple-data/src/model/`, `src/query/` | Rust data layer |
| `crates/dapple-prompt-api/src/handler/` | API endpoint handlers (ingest, search, render) |

## Cross-book pointers

| You want to… | Go to |
|--------------|-------|
| Call the API to ingest, search, or run journeys | [`../api-consumer-guide/AGENTS.md`](../api-consumer-guide/AGENTS.md) |
| Understand parser/ingest internals or extend them | [`../journey-builder-guide/AGENTS.md`](../journey-builder-guide/AGENTS.md) |
| Look up shared concepts (auth, data model, briefs) | [`../_foundation/`](../_foundation/) |
| Understand audit/billing surfaces (mostly WIP) | [`../journey-auditor-guide/AGENTS.md`](../journey-auditor-guide/AGENTS.md) |
