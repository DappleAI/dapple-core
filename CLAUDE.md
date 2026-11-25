# Claude Code Instructions — dapple-prompt

This document provides comprehensive guidance for working with the dapple-prompt codebase using Claude Code.

## Shell — Always Use Nushell

**CRITICAL: ALL shell commands MUST be run through `nu -c '...'`.** Never run bare bash commands. The entire development environment — `DAPPLE_PROMPT_DATABASE_URL`, `KUBE_DAPPLE_NAMESPACE`, kubectl, cargo, curl, psql, environment overlays, deployment tooling — lives in Nushell. Bash is a bare shell with none of this configured. Every Bash tool invocation must wrap the command in `nu -c`.

```bash
# CORRECT — always wrap in nu -c
nu -c 'cargo build -p dapple-prompt-api'
nu -c 'cargo test --workspace'
nu -c 'kubectl get pods -n dapple-sandbox'
nu -c 'curl -s http://localhost:8001/api/guides'
nu -c 'psql $env.DAPPLE_PROMPT_DATABASE_URL -c "SELECT 1"'

# WRONG — bare bash has nothing: no env vars, no kubectl, no paths
cargo build             # ← may lack DAPPLE_PROMPT_DATABASE_URL, SQLX_OFFLINE, etc.
kubectl get pods        # ← command not found
curl -s http://...      # ← may lack env-dependent URLs
```

For multi-line or complex commands:
```bash
nu -c '
  let pod = (kubectl get pods -n dapple-sandbox -l app=dapple-prompt-api -o name | str trim)
  kubectl logs $pod -n dapple-sandbox --tail=50
'
```

For deployment operations, invoke Nushell scripts directly:
```bash
nu assets/nu/k8s/deploy_dapple_core.nu --svc api
```

## Documentation

Narrative documentation lives in `docs/books/` (renamed from `docs/guides/` because "Guide" is a first-class platform entity — see `dapple-parser` and `rev.guides`). Start with `docs/books/README.md`; each book has an `AGENTS.md` aimed at agents being dropped in cold. Shared content used by multiple books lives in `docs/books/_foundation/`. Architecture plans (historical and in-flight) live in `docs/plans/`.

## Workspace Overview

**Monorepo structure** with multiple Rust crates under `crates/` each with an AGENTS.md file for additional instructions:

## Crates And Responsibilties
### dapple-data
#### Responsibilities
Data access layer to PostgreSQL via SQLx, All SQL queries, Data Models, Queries and Data Access Philosophies. Uses **sql-composer** (`cargo sqlc`) to compose `.sqlc` source files into generated `.sql` output — never edit `assets/.sql/` directly.
#### Agent Information
Explore @crates/dapple-data/AGENTS.md for all details related to crate responsibilities.

### dapple-fluent
#### Responsibilities
Fluent Template Parsing, Rendering and Modeling
#### Agent Information
Explore @crates/dapple-fluent/AGENTS.md for all details related to crate responsibilities.

### dapple-parser
#### Responsibilities
Guide content parser (markdown directives, TOML metadata, Fluent messages, SUMMARY.md) - @crates/dapple-parser/AGENTS.md
#### Agent Information
Explore @crates/dapple-parser/AGENTS.md for all details related to crate responsibilities.

### dapple-prompt-api
#### Responsibilities
REST API service (Axum handlers, routes, OpenAPI) acting as the **internal-tier** interface for the dapple-prompt system on `api.dapple.x`. Used by dapple-team services (dapple-ui, conduit, flow). Single-tenant by design — does not filter queries by company / namespace. Includes guide ingest with async embedding generation, guide search endpoint, and classification filtering.
#### Agent Information
Explore @crates/dapple-prompt-api/AGENTS.md for all details related to crate responsibilities.

### dapple-prompt-external-api
#### Responsibilities
**External-tier** REST API surface served on `external-api.dapple.x` for customer companies (third parties such as Acme). Mounted as a separate Axum router with its own extractor (`ExternalCaller`), its own service-token table (`rev.external_service_tokens`), and its own JWT signing key (`DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY`). Cryptographically isolated from `dapple-prompt-api`: a JWT signed for one surface cannot be verified by the other. Every query filters by the caller's `company_id` so cross-namespace visibility is structurally impossible.
#### Agent Information
Explore @crates/dapple-prompt-external-api/AGENTS.md for all details related to crate responsibilities.

### dapple-prompt-conduit
#### Responsibilities
Prompt execution conduit — LLM backends (Anthropic, Mistral), embedding generation (ModernBERT/ONNX), RAG retrieval, TTS, summarization. Hosts `/embed` endpoint for vector generation and `/prompt` with optional RAG context injection.
#### Agent Information
Explore @crates/dapple-prompt-conduit/AGENTS.md for all details related to crate responsibilities.

### dapple-prompt-flow
#### Responsibilities
Workflow engine for processing Journey Dispatches
#### Agent Information
Explore @crates/dapple-prompt-flow/AGENTS.md for all details related to crate responsibilities.

### dapple-prompt-mock-api
#### Responsibilities
#### Agent Information
Explore @crates/dapple-prompt-mock-api/AGENTS.md for all details related to crate responsibilities.

### dapple-render
#### Responsibilities
Content Rendering (markdown-to-HTML, PDF generation, tag filtering) - @crates/dapple-render/AGENTS.md
#### Agent Information
Explore @crates/dapple-render/AGENTS.md for all details related to crate responsibilities.

## Journey-Centric Philosophy

This project follows a journey-centric design philosophy. Understanding this philosophy is essential for making correct architectural decisions.

Users experience a **Playbook** (the workflow they pick) → **Journey** (their session) → **Debrief** (the interview) → **Dispatch** (background execution). Internal objects (Brief, BriefPack, value slots, dispatches) are not user-facing.

**Stages are panes; steps are the AJAXy interactions on a pane.** A **stage** is a static *form / pane* the user lands on; advancing to the next stage (stage N → stage N+1) is the real navigation to the next pane. **Steps** are the dynamic, AJAXy sub-interactions that happen *within* a pane before the user navigates on: the canonical shape is **generate → pick → refine** (the task generates candidates, the operator picks one, a follow-up step refines it). **Multi-step stages are the norm, not the exception** — that is what steps are *for*. A single-step stage is the degenerate case (a pane with one round-trip), not the model.

Consequences that the data model and authoring follow from this:

- **A step is an ordered, fail-together set of tasks** (a coherence + retry boundary, NOT a DB transaction). Multiple tasks per step is the norm; data retrieval/fetch is a task *inside* a step, never its own step. Tasks run in authored order; the step **stops at the first task failure** (or a `signals.unsafe = true` veto → terminal `unsafe` phase, non-retryable). Nothing rolls back — attempts/payloads/assets are recorded as history; retry is a re-dispatch (latest-wins). Authors write a step's tasks as **nested `:::nushell` children** (step-private, auto-scoped) and/or a **`tasks: [...]`** attribute (shared refs). See `crates/dapple-prompt-flow/AGENTS.md` (Step execution model).
- **Per-step status is the fine-grained completion signal.** `GET /api/journeys/{id}/steps` (each entry's `done` + `exposed_slots`) tells a client *which* sub-interaction in a stage is awaiting input; `/stages` is the coarse roll-up (a stage is done when all its steps are). Consumers (dapple-ui, the dapple-suite runner, external customers) poll these.
- **Overlays (`expose`/`mask`) are authored per node.** A slot a *specific* step needs or produces is exposed at the **step** level (`:::step { expose: [...] }`), not blanket-exposed at the stage. Reach for stage-level `expose` only for a slot the *whole* stage shares.
- **Computed outputs are produced by a step; bindings are write-once per dispatch.** A `:::brief-output { source_kind: "computed" }` is bound by the runtime once the producing step **coheres** (every task succeeds, no `unsafe` veto) — value extracted by `source_path` from the step's payload. A failed/vetoed step publishes nothing. A **dispatch is a batch of one-or-more steps** — you may fire a whole stage as one dispatch. To point at the producing step precisely in a multi-step stage, expose the output at the **step** level (or give it a named `source_path`); `source_path: "."` (whole payload) is for a single computed output per stage. A binding is immutable within a dispatch (first producing step wins, like an operator answer); to change a value you re-dispatch (latest-wins across dispatches). Don't reintroduce an "ownership gate" that refuses stage-level computed exposes on multi-step stages. See `crates/dapple-prompt-flow/AGENTS.md` and `docs/books/journey-planner-guide/value-slot-overlays.md`.
- **Binding *reads* are journey-wide and order-independent.** Any task reads every value-slot binding the journey and its predecessor chain hold (latest-wins, `depth ASC, created_at DESC`), via `$in.bindings.*` / `select_effective_bindings_for_journey`. Per-step *dispatch* and per-stage/per-step *DONE* rollups scope completion/production, **never reads** — running stages out of order (B→A→C) still lets a later stage read an earlier one's values. A value invisible across stages is a producer that didn't write the binding, or a *consumer* (e.g. a UI resolver) scoping to the active stage — not a backend gap. See `crates/dapple-prompt-flow/AGENTS.md` → "Binding read availability".

## Conduit Context Frames

Every dispatch-backed LLM prompt carries a layered **frame stack** (final
system-message order): `[platform_frame, guide_frame (# Guide Frame),
task_context (# Dispatch Context), rag (# Reference Context), ...template,
user]`. The theme is **unclouding the conduit's context** — purpose-built
frames in, distractors out (signal quality, not cost):

- **CONDUITS.md is a required root-level file in every guide** — the
  conduit-facing guide frame (binding semantics, how to read agent prose),
  stored per guide version (`rev.guides.conduits_md`) and injected whole. It
  is the sibling of a guide's AGENTS.md (authoring-agent context, never
  ingested); deriving one surfaces gaps to backfill into the other.
- **Overlay blinders**: if a task's stage→step→task overlay cascade has any
  overlays, the conduit's task-context bindings are filtered to the exposed
  set (server-side in `resolve_task_context`). No overlays ⇒ unfiltered.
  Engine-side `$in.bindings` is NEVER filtered (flow invariant).
- **Neutral RAG headers**: retrieved excerpts are labeled `## Excerpt i/N` —
  audience-voiced guide/chapter titles never reach the conduit.

See `crates/dapple-prompt-conduit/AGENTS.md` (frame stack),
`crates/dapple-prompt-api/AGENTS.md` (task_context + blinders),
`crates/dapple-parser/AGENTS.md` (required files), and
`docs/plans/conduit-context-frames.md` (taxonomy + deferred journey frame).

## Development Workflow

### Building
```bash
# Format and lint
cargo fmt
cargo clippy -- -D warnings

# Build workspace
cargo build --workspace

# Build specific crate
cargo build -p dapple-data
```

**hostPath deploys build for you.** A hostPath pod runs the binary mounted from
`target/<profile>/`, so a stale artifact deploys green while the new code never
runs. `deploy_dapple_core.nu` therefore compiles every hostPath service
itself before mounting it (`cargo build` on Linux, `cargo zigbuild` on macOS) —
a forgotten `cargo build --workspace` can no longer ship the wrong version. Pass
`--no-build` to mount `target/` as-is.

#### macOS hostPath builds (cross-compile to Linux)

On macOS, hostPath deploys mount a cross-compiled `aarch64-unknown-linux-gnu`
binary into the OrbStack VM (Mach-O can't run there). The deploy script
auto-runs `cargo zigbuild` for cross-compile-eligible services — `api`,
`external-api`, `flow` — so the workflow is identical on Mac and Linux:

```bash
# One-time toolchain install (handled by setup.nu):
#   rustup target add aarch64-unknown-linux-gnu
#   brew install zig
#   cargo install cargo-zigbuild

# Then just deploy — the script builds the right artifact for the host:
nu assets/nu/k8s/deploy_dapple_core.nu --svc api
```

Conduit-family services (`conduit`, `conduit-onnx`, `conduit-llm`) auto-promote
to Docker mode on macOS because their third-party native deps (`mistralrs`,
`hf-hub`, `ort`) can't cross-compile cleanly. Pass `--docker dev` to opt every
service into Docker mode regardless of host.

### sql-composer (SQL Query Authoring)

**CRITICAL: Never edit files in `crates/dapple-data/assets/.sql/` directly.** The `.sql/` directory is generated output from sql-composer. All SQL authoring happens in `assets/sqlc/` using `.sqlc` source files.

```bash
# Preferred: compose .sqlc -> .sql AND refresh the sqlx offline cache in one step:
nu -c 'use assets/nu/modules/dapple; dapple sqlc compose'

# CI drift check (does generated .sql match .sqlc sources?):
nu -c 'use assets/nu/modules/dapple; dapple sqlc verify'

# Raw equivalents (what the wrapper runs under the hood):
nu -c 'cd crates/dapple-data; cargo sqlc compose --source assets/sqlc/ --target assets/.sql/'
nu -c 'cd crates/dapple-data; cargo sqlc compose --verify --source assets/sqlc/ --target assets/.sql/'
nu -c 'cargo sqlx prepare --workspace -- --all-targets'
```

`dapple sqlc compose` runs the compose from the dapple-data crate and then
`cargo sqlx prepare --workspace -- --all-targets` (the generated queries are referenced from
every prompt crate, so a crate-local prepare would leave the others' offline
cache stale). Run `nu assets/nu/init/setup.nu --environment sandbox` once so `dapple sqlc` is on
your PATH in interactive shells.

**Comments: use `#`, not `--`.** sql-composer strips `#` comments from the
generated `.sql` (they document the source and never reach the database) but
passes `--` comments through verbatim to Postgres. Document `.sqlc` files with
`#`; reserve `--` for the rare case you actually want a comment in the
DB-facing SQL (almost never). Both leading and inline `#` comments are stripped;
an inline `#` also consumes its line's newline, so a trailing-comment line may
merge with the next in the generated output — harmless, since whitespace is
insignificant in SQL outside string literals.

**Key rules:**
- `.sqlc` files use `:bind(name)::TYPE` for parameters and `:compose(path.sqlc)` to inline shared CTE fragments
- **Shared `.sqlc` files are COMPLETE queries only** — never clause fragments or parenthesized expressions; the call site owns all wrapper syntax (parens, `LEFT JOIN LATERAL (...) x ON TRUE`, `cte AS (...)`). See `crates/dapple-data/AGENTS.md` → "Shared fragments are COMPLETE queries only".
- **Slots:** `:compose(@name)` declares a named slot in a template; callers fill it via `:compose(body.sqlc, @name = filler.sqlc)`. SELECT queries use this to share body templates across variants (by_object_ids, by_slugs, all, etc.) with only the page CTE differing.
- sql-composer assigns `$N` placeholders by **alphabetically sorting** all `:bind()` names across the composed query (including slots and composed-in files)
- Rust `query_file_as!` parameters must be passed in this same alphabetical order
- Shared CTE fragments live in `assets/sqlc/shared/` and are composed into entity queries
- See `crates/dapple-data/AGENTS.md` for the full sql-composer workflow and shared statement inventory

## Infrastructure Overiew

### Nushell Tooling
#### Overview
Everything revolves around our Nushell Tooling and is ex
#### Agent Information
Explore @assets/nu/AGENTS.md

### K8s Setup
### Overview
Locally environment runs in a microk8s kube. macOS contributors run microk8s inside an OrbStack-hosted Ubuntu VM — see @docs/books/journey-builder-guide/development/orbstack-setup.md for setup.
#### Agent Information
Explore @assets/k8s/AGENTS.md

## Environment Variables

### Core
- `DAPPLE_PROMPT_DATABASE_URL` — PostgreSQL connection string for prompt services (required). Points to the `prompt-db` K8s service.
- `SQLX_OFFLINE` — Set to `true` for offline query verification
- `RUST_LOG` — Logging level (e.g., `debug`, `info`)

### LLM Backends (dapple-prompt-conduit)
- `ANTHROPIC_API_KEY` — Anthropic API key for Claude models
- `MISTRAL_ENABLED` — Set to `true` to enable local Mistral inference
- `MISTRAL_MODEL_ID` — HuggingFace model ID (default: `mistralai/Mistral-7B-Instruct-v0.3`)
- `MISTRAL_DEVICE` — Device for inference: `cpu`, `cuda:0`, `metal` (auto-detected if not set)
- `MISTRAL_DTYPE` — Data type: `f16`, `bf16`, `f32` (default: `f16`)
- `MISTRAL_MAX_CONTEXT` — Max context length (default: `8192`)
- `HF_TOKEN` — HuggingFace token for gated models

### Embedding & Search (dapple-prompt-conduit)
- `EMBEDDING_ENABLED` — Set to `true` to enable embedding service (default: `false`)
- `EMBEDDING_MODEL_ID` — HuggingFace model ID (default: `lightonai/modernbert-embed-large`)
- `EMBEDDING_DIMENSION` — Vector dimension (default: `768`)
- `EMBEDDING_REQUIRED` — Panic on initialization failure (default: `false`)

### API-Conduit Integration (dapple-prompt-api)
- `DAPPLE_PROMPT_CONDUIT_URL` — Conduit service URL for embeddings and search (e.g., `http://dapple-prompt-conduit`). When unset, guide search and embedding generation are silently disabled.

### External API Surface (dapple-prompt-external-api)
- `DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY` — JWT signing key for the external surface. **Distinct** from `DAPPLE_PROMPT_API_SIGNING_KEY`; sharing keys breaks the cryptographic-isolation guarantee. Missing key = fail-fast at startup.
- `DAPPLE_PROMPT_EXTERNAL_API_PORT` — Listener port (default 8081).
- `EXTERNAL_JWT_EXPIRY_SECS` — JWT lifetime in seconds (default 3600).
- `DAPPLE_COMPANY_UUID` — The dapple system org's fixed UUID (`019e132e-2610-7a4a-b64a-77a49e286ac0`). Set automatically by the nushell environment overlay; available to scripts and tests that need to reference dapple's namespace.

## Multi-Surface Architecture

The system runs as **three logical API surfaces**, each fronted by its own K8s ingress and (in V1) its own binary:

| Surface | Hostname | Token table | Signing key | Crate |
|---|---|---|---|---|
| Internal | `api.dapple.x` (cluster-internal) | `web.service_tokens` | `DAPPLE_PROMPT_API_SIGNING_KEY` | `dapple-prompt-api` |
| External | `external-api.dapple.x` (public) | `rev.external_service_tokens` | `DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY` | `dapple-prompt-external-api` |
| User (future) | `user-api.dapple.x` | TBD | TBD | reserved |

**Cryptographic isolation:** A JWT minted on one surface cannot be verified by another — verification fails at signature check before any claim is read. This is not just an `aud`-claim convention; the keys are different.

**Data isolation:** Path-bearing tables (`rev.object_paths`, `rev.canonical_release_pins`) carry a `namespace_company_id UUID FK` to `rev.companies`. The external surface filters every query by the caller's `company_id`; cross-namespace visibility is structurally impossible.

**Org primitive:** `rev.companies` is the canonical org table. The dapple system org has a fixed UUID seeded by Migration 31 (also exported as `DAPPLE_COMPANY_UUID` in the nushell env). Customer onboarding (Migration 34) inserts a new `rev.companies` row + a `rev.external_service_tokens` row with the customer's Ed25519 public key.

**Scope tiers:** `scope` on `object_paths` and `canonical_release_pins` is a flat enum: `public`/`common`/`internal` (dapple namespace only) and `external` (non-dapple namespaces only). The `external` mirror tier is for customer-private content. See `crates/dapple-data/AGENTS.md` (Multi-Tenancy Primitives) for the full schema and `crates/dapple-prompt-external-api/AGENTS.md` for surface invariants.

## Content-Addressable Versioning

Versioned objects carry a `content_hash` (SHA-1) on `rev.versioned_objects` for deduplication. The `ContentHash` newtype in `dapple-data` wraps a 40-char hex string with a `.short()` method for 8-char display. Hash functions in `dapple-data::hash` cover all entity types. Playbooks, stages, and steps use a Merkle pattern where parent hashes include sorted child hashes, so any descendant change propagates upward. Guide directories are hashed from tarball contents via `hash_guide_directory()`. On ingest or PATCH, if the computed hash matches the latest version, the operation returns the existing version without creating a duplicate. See `crates/dapple-data/AGENTS.md` for the full hash function inventory.

**Reference exact versions; don't bake copies or resolve latest-by-path.** When one versioned object references another for display or composition, pin the exact version id — never bake a text copy of the other object's content, and never store/resolve the relationship by path/name/slug (= "latest", which drifts). Display text follows the brief-unit hybrid: a nullable `*_text` fallback column **plus** a `*_fluent_message_id` reference, read via `COALESCE(rev.fluent_body(ref, 'en'), text, name)`. Because the model is append-only, an entity that references a Fluent message for display must fold that message's `content_hash` into its own hash, so a Fluent-body edit re-versions the entity and re-pins the new version. Path/name/slug resolution is reserved for upsert/lineage, browse/catalog, pin-latest-**once**-at-create, and forward write targets. Full rationale: `crates/dapple-data/AGENTS.md` → "Versioned object references — pin exact versions".
