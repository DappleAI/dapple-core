# dapple-prompt Books

This directory contains the long-form books that document the dapple-prompt platform. Each book is an independent mdBook project targeted at a specific audience.

## Why "books" and not "guides"?

**"Guide" is a first-class platform entity.** A *Guide* is what `dapple-parser` ingests, what lives in the `rev.guides` table, what renders to PDF via render profiles, and what materializes into Playbooks, Briefs, Tasks, and Inference Profiles on ingest. To stop conflating the platform concept with the documentation directory, the docs that *describe* the platform live here in `docs/books/`. The platform's own concept of a Guide is documented inside `journey-planner-guide/` (which itself is a book about the Guide system). Yes, this is confusing — the rename is the fix.

## The Books

| Book | Audience | Read it for |
|------|----------|-------------|
| [`api-consumer-guide/`](./api-consumer-guide/) | **Internal** API consumers, dapple-team integration developers | REST/SDK usage, authentication, endpoint reference, ingest API for the **internal** surface (`api.dapple.x`) |
| [`external-api-consumer-guide/`](./external-api-consumer-guide/) | **External/customer** companies (third parties) | The customer-tier surface (`external-api.dapple.x`): Ed25519 → JWT auth, per-company isolation, the narrower V1 endpoint set, guide ingest, automatic dispatch execution |
| [`journey-planner-guide/`](./journey-planner-guide/) | Prompt engineers, guide authors, agents | Authoring Guides: directives, Fluent localization, Nushell tasks, ingest pipeline |
| [`journey-builder-guide/`](./journey-builder-guide/) | Backend / platform engineers | Internals: parser, data layer (sql-composer), flow engine, conduit, K8s deploy |
| [`context-engineering-review/`](./context-engineering-review/) | Researchers, architects | Comparative essay against the *Everything is Context* paper — analysis, not how-to |

Each book has an `AGENTS.md` at its root that an agent should read before any chapter.

## Special Directories

| Directory | Purpose |
|-----------|---------|
| [`_foundation/`](./_foundation/) | Shared content included by multiple books via `{{#include}}`. Glossary, authentication docs, API patterns, concepts, data models, theme assets. Changes here propagate. |
| [`_locales/`](./_locales/) | Locale-specific Fluent files (`en/`, `pt-BR/`, `pt-PT/`) for translated or region-specific material. |

## Start here, agent

If this is your first contact with the codebase, read this before opening any book or running any tool. These are the rules people most often violate; obeying them saves hours.

1. **Always run shell commands through `nu -c '...'`.** Bare bash has no `DAPPLE_PROMPT_DATABASE_URL`, no `kubectl`, no nu modules, no environment overlays. Every `Bash` tool call must wrap its command in `nu -c`.
2. **Never edit `crates/dapple-data/assets/.sql/` directly.** Those files are generated. Author `.sqlc` sources in `assets/sqlc/`, then run `cargo sqlc compose --source assets/sqlc/ --target assets/.sql/`.
3. **sql-composer assigns `$N` placeholders alphabetically** across all `:bind()` names in the composed query. Rust `query_file_as!` parameters must be passed in that same alphabetical order. Check the generated `.sql` file when in doubt.
4. **Run migrations with `cargo sqlx migrate run`, never `psql -f`.** The migration table tracks what's been applied; bypassing it corrupts state.
5. **Speak the journey-centric vocabulary.** Users see *Playbook* (workflow they pick) → *Journey* (their session) → *Debrief* (interview experience) → *Dispatch* (background execution). Internal code may still use `Brief`; user-facing surfaces say *Debrief*.
   - **Stages are panes; steps are the interactions on them.** A *stage* is a static form / pane the user advances *between*; *steps* are the dynamic, AJAXy sub-interactions *within* a pane (generate → pick → refine) before navigating to the next stage. Multi-step stages are the norm. Value-slot overlays (`expose`/`mask`) are authored per node — a slot a specific step needs or *produces* (a computed output) goes on the `:::step`, not blanket on the `:::stage`.
6. **The data layer is append-only.** Versioned objects use Path tables and version triplets; deletes are soft (deprecation events). No UPDATE in normal flow.
7. **Versioned objects are content-addressable.** SHA-1 over canonical content; on ingest/PATCH, a matching hash returns the existing version (no duplicate). Parents Merkle-hash sorted child hashes.
8. **Guides are Fluent-native.** Localizable directive blocks (prose, stage, step, brief_pack, inference_profile, block) carry `content: None`; their text lives in `.ftl` files under the guide's `locales/`. Inline blocks (nushell, ref) keep their content.
9. **Local deploy mode is hostPath, not Docker.** `nu assets/nu/k8s/deploy_dapple_core.nu --svc api` mounts your local `target/debug/` binary into the pod. `--docker dev` is opt-in and refuses to run on a dirty git tree.
10. **Two engines named "Engine" — don't conflate them.** `StepEngine` (in `dapple-prompt-flow`) parses and runs Nushell scripts with security policy. `dataflow_rs::Engine` orchestrates JSONLogic-driven workflows. Same word, different jobs.

For per-crate detail, every crate has its own `AGENTS.md`. The terse rules above mostly come from `CLAUDE.md` and the `crates/*/AGENTS.md` files; this list is the survival kit.

## What changed recently

These are the shifts that have hit the system in the last several months. If your mental model predates them, recalibrate before reading further.

- **sql-composer DSL** — Hand-written SQL replaced by `.sqlc` sources composed via `cargo sqlc compose`. Slot pattern (`:compose(@page)`) for SELECT variants. Alphabetical bind ordering.
- **Fluent-native architecture** — Guides are locale-independent; localizable text lives in `.ftl` files. Most directive blocks are `content: None` and resolved through Fluent message keys.
- **Render profiles** — Tag-based PDF filtering with stored profiles. Tags: `agent`, `product`, `coder`, `auditor`, `planner`. Untagged blocks always included. Three branding styles.
- **Brief packs / brief units** — Multi-unit briefs (markdown, notice, input, output) tied to chapters/blocks; richer than the old single-content brief.
- **pgvector RAG** — `rev.guide_content_embeddings` (768-dim ModernBERT). `GET /api/guides/search`. Optional context injection in `/prompt`. Embedding generation is fire-and-forget after ingest.
- **Content-addressable hashing** — `ContentHash` newtype; SHA-1 over canonical content. Merkle propagation for parents. Ingest dedups against the latest version's hash.
- **Conduit decomposition** — `dapple-prompt-conduit` routes by `lineage_key` to specialized services: `LlmService` (LLM backends), `EmbeddingService` (`/embed`), `MlmService`, `SummarizationService`, `TtsService`, `RagService`. ONNX backends for ModernBERT and others.

## Building the books

Each book is an independent mdBook project.

```bash
nu -c 'cd docs/books/api-consumer-guide; mdbook build'
nu -c 'cd docs/books/api-consumer-guide; mdbook serve --open'
```

The rendered output lands in the book's `book/` directory.

The docs container (`docs/service/Dockerfile.docs`) builds every book and serves them under `/srv/guides/...` (the public URL prefix is intentionally still `/guides/` to avoid breaking existing links — only the source directory was renamed).

## Adding a new book

1. Create `docs/books/{audience}-{topic}/` (or whatever name maps to the audience).
2. Add `book.toml`, `SUMMARY.md`, `preface.md`, and any chapter files.
3. **Mandatory:** add an `AGENTS.md` at the book's root that follows the five-section template — see any existing book's AGENTS.md for the shape (`api-consumer-guide/AGENTS.md` is the cleanest reference).
4. Use `{{#include ../_foundation/...}}` for content that should stay in sync with other books.
5. Add an entry to the table above.
6. Add the build steps to `docs/service/Dockerfile.docs` if the book should ship in the docs container.
