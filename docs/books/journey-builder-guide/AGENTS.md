# Agent Reading Guide — Journey Builder Guide

## What this book is for

This book is the internal architecture reference for the dapple-prompt backend: parser internals, the data layer (sql-composer), the flow engine, the conduit, the SDK code generation, K8s deployment, and the render pipeline. Read this if you are extending the platform itself.

This book is **not** for guide authors (see `journey-planner-guide/`) or API consumers (see `api-consumer-guide/`).

## Read this first if…

| You're trying to… | Start here |
|--------------------|------------|
| Get a top-down architecture view | `getting-started/architecture-overview.md` |
| Add a new principal/versioned entity | `data-layer/query-patterns.md`, `data-layer/model-patterns.md`, then `guide-system/extending.md` |
| Author SQL (`.sqlc` sources, slot pattern, `cargo sqlc compose`) | `data-layer/query-patterns.md` |
| Add or modify a directive in the parser | `guide-system/parser-architecture.md` → `guide-system/transform-layer.md` → `guide-system/extending.md` |
| Understand or extend the ingest transaction | `guide-system/materialization.md` |
| Add a new render profile or branding style | `guide-system/render-pipeline.md` |
| Add a Nushell command, plugin, or function | `nushell/command-authoring.md`, `flow/security-model.md` |
| Add a new LLM backend or embedding backend | `conduit/llm-backends.md`, `conduit/embedding-backends.md`, `conduit/model-taxonomy.md` |
| Wire a new API endpoint | `getting-started/architecture-overview.md`, then `crates/dapple-prompt-api/AGENTS.md` |
| Generate / modify the SDK | `sdk/overview.md`, `sdk/adding-resource-clients.md` |
| Deploy locally, debug a pod, or change manifests | `kubernetes/deployment-scripts.md`, `kubernetes/dapple-cli.md` |
| Understand StepEngine vs `dataflow_rs::Engine` | `flow/overview.md` (the chapter calls this out explicitly) |

## Concepts you must already understand

The terse versions live in `docs/books/README.md` ("Start here, agent"); the canonical versions are in `CLAUDE.md` and the per-crate `AGENTS.md` files. The most violated rules for backend work:

- **Always run shells via `nu -c '...'`.** Bare bash has no `DAPPLE_PROMPT_DATABASE_URL`, no `kubectl`, no nu modules.
- **Never edit `crates/dapple-data/assets/.sql/` directly.** SQL is authored in `assets/sqlc/` (`.sqlc` sources) and compiled with `cargo sqlc compose --source assets/sqlc/ --target assets/.sql/`.
- **`:bind()` parameters are alphabetically ordered.** sql-composer assigns `$N` placeholders by sorting all bind names across the composed query (including composed-in shared files). Rust `query_file_as!` parameters must be passed in that same alphabetical order. When in doubt, read the generated `.sql` file.
- **Append-only, soft delete, content-addressable.** No UPDATE in normal flow; deletes go through deprecation; versioned objects carry a SHA-1 hash, and a matching hash short-circuits on PATCH/ingest.
- **Migrations run via `cargo sqlx migrate run`, never `psql -f`.**
- **Two engines named "Engine" — don't conflate them.** `StepEngine` (in `dapple-prompt-flow`) parses and runs Nushell scripts. `dataflow_rs::Engine` (from the `dataflow-rs` crate) orchestrates JSONLogic-driven workflows. Same word, different jobs. Cited everywhere; flagged in `flow/overview.md`.
- **Journey-centric vocabulary.** Internal types are still named `Brief`/`BriefPack`/`BriefGroup`; user-facing surfaces and most prose say *Debrief*. Don't fix this in code as a side-effect.
- **Default deploy mode is hostPath, not Docker.** `nu assets/nu/k8s/deploy_dapple_prompt_pods.nu --svc api` mounts your local `target/debug/` binary into the pod. `--docker dev` is opt-in and refuses to run on a dirty git tree.
- **`assets/.sqlx/` is the offline metadata cache.** Set `SQLX_OFFLINE=true` for CI. Run `dapple sqlx prepare` (raw: `cargo sqlx prepare --workspace -- --all-targets`) after editing `.sqlc` and re-running `cargo sqlc compose` — `--all-targets` keeps the cache covering test-target inline queries, which CI's offline build needs.
- **Embedding generation is async.** The API ingest handler spawns a background task to call the conduit's `POST /embed` and write `rev.guide_content_embeddings`. Failures log a warning and never fail the ingest.

## Quick navigation

| Area | Chapter | Notes |
|------|---------|-------|
| Architecture | `getting-started/architecture-overview.md` | Component diagram, data flow |
| Local dev | `development/local-development.md`, `development/debugging.md` | StepEngine debug, parse-cache |
| **Parser** | `guide-system/parser-architecture.md`, `transform-layer.md`, `materialization.md`, `data-model.md`, `extending.md`, `render-pipeline.md` | The whole guide pipeline |
| **Data layer** | `data-layer/query-patterns.md`, `model-patterns.md`, `migration-guide.md` | sql-composer, slot pattern, alphabetical binds |
| **Flow** | `flow/overview.md`, `step-engine.md`, `nushell-execution.md`, `dataflow-integration.md`, `security-model.md` | Two engines named Engine; security policy |
| **Conduit** | `conduit/overview.md`, `llm-backends.md`, `embedding-backends.md`, `model-taxonomy.md`, `overlay-system.md`, `template-rendering.md` | LLM routing by `lineage_key`, embeddings, prompt rendering |
| **Nushell** | `nushell/command-authoring.md`, `custom-commands.md` | Adding `prompt send`, `summarize`, `madlib`, etc. |
| **SDK** | `sdk/overview.md`, `adding-resource-clients.md` | typify-based code generation, OpenAPI |
| **Kubernetes** | `kubernetes/deployment-scripts.md`, `dapple-cli.md` | hostPath default, Docker `--docker dev` opt-in |
| **Testing** | `development/testing-patterns.md` | ScenarioContext, lifecycle trials |

## Recently added or shifted (read here before assuming chapters are current)

- **`conduit-llm` and `conduit-onnx` are separate binary crates** under `crates/`, decomposed out of the monolithic conduit so model inference runs in dedicated services. The main `dapple-prompt-conduit` still hosts the public `/prompt`, `/embed`, `/summarize`, `/synthesize` endpoints; specialized inference (mistral.rs GGUF, ONNX) is delegated to those sub-services. If a conduit chapter still talks as if everything runs in one process, treat it as describing the routing layer, not the deployment topology.
- **Lago billing.** A self-hosted Lago instance handles subscription/usage billing. The Rust side lives in `crates/dapple-lago` (client library) and `crates/dapple-prompt-lago-worker` (background worker). Deployment is `nu assets/nu/k8s/deploy_lago_billing.nu`; manifests are under `assets/k8s/registry/lago/`. There is currently no dedicated chapter for Lago — read those crates' source and the deploy script directly.
- **pgvector RAG pipeline.** `rev.guide_content_embeddings` (768-dim ModernBERT vectors) plus tag/locale junctions. `GET /api/guides/search` runs through `EmbeddingService` (in the conduit) → pgvector cosine distance. Optional `RagService` injects retrieved chunks into prompts as a system message. Chapters: `conduit/embedding-backends.md`, `guide-system/data-model.md` (search section). Embedding generation on ingest is fire-and-forget.
- **Render profiles.** Tag-based filtering is now first-class: `rev.render_profiles` rows define `tag_filter` + `style_config`. Tags include `agent`, `product`, `coder`, `auditor`, `planner`. Untagged blocks always pass through. See `guide-system/render-pipeline.md`.
- **Content-addressable hashing.** `dapple-hash` crate is now the home of `ContentHash` and per-entity hash functions. The Merkle pattern propagates child hash changes upward.

## Source-file references

| Path | What lives there |
|------|------------------|
| `crates/dapple-data/assets/sqlc/` | SQL **sources** (edit these) |
| `crates/dapple-data/assets/.sql/` | SQL **generated output** (never edit) |
| `crates/dapple-data/src/model/`, `src/query/` | Rust models and query functions |
| `crates/dapple-parser/src/` | Fence tokenizer, block classifier, transform layer |
| `crates/dapple-fluent/src/` | Fluent message parsing, message-set grouping |
| `crates/dapple-hash/src/` | `ContentHash`, per-entity Merkle hash functions |
| `crates/dapple-prompt-api/src/handler/`, `src/route/`, `src/schema/` | API endpoints |
| `crates/dapple-prompt-flow/src/engine/` | StepEngine + security policy |
| `crates/dapple-prompt-conduit/src/handler/`, `src/service/` | LLM/embedding/RAG routing |
| `crates/conduit-llm/`, `crates/conduit-onnx/` | Sub-service binaries for inference |
| `crates/dapple-render/src/` | PDF renderer, render-profile filtering |
| `crates/dapple-sdk/` | Typed SDK + OpenAPI artifact |
| `crates/dapple-lago/`, `crates/dapple-prompt-lago-worker/` | Lago billing client + worker |
| `assets/nu/k8s/` | Deploy scripts |
| `assets/k8s/manifests/`, `registry/` | K8s templates and per-service manifests |

## Cross-book pointers

| You want to… | Go to |
|--------------|-------|
| Author a Guide directory (markdown directives, Fluent, guide.toml) | [`../journey-planner-guide/AGENTS.md`](../journey-planner-guide/AGENTS.md) |
| Call the platform from outside (SDK, REST) | [`../api-consumer-guide/AGENTS.md`](../api-consumer-guide/AGENTS.md) |
| Read shared platform concepts | [`../_foundation/`](../_foundation/) |
| See per-crate quick references | `crates/*/AGENTS.md` (each crate has one) |
| See the project-level invariants | [`../../../CLAUDE.md`](../../../CLAUDE.md) |
| See platform-wide doc rules of the road | [`../README.md`](../README.md) |
