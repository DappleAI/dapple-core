# Dapple Prompt

A Rust-based monorepo for building workflow-driven prompt engineering systems with PostgreSQL persistence and Nushell-based task execution.

## Overview

Dapple Prompt provides a comprehensive platform for managing structured prompts, playbooks, journeys, and workflows. The system is designed around principles of data integrity, batch operations, and immutable versioning.

## Architecture

This is a Cargo workspace containing the following crates:

- **[dapple-data](crates/dapple-data/README.md)** — Data access layer with SQLx, PostgreSQL models, and query functions
- **[dapple-prompt-api](crates/dapple-prompt-api/README.md)** — REST API service built with Axum, exposing OpenAPI 3.1 endpoints
- **[dapple-prompt-conduit](crates/dapple-prompt-conduit/README.md)** — Prompt rendering and LLM routing service with multi-backend support
- **[dapple-prompt-flow](crates/dapple-prompt-flow/README.md)** — Workflow execution engine with Nushell integration
- **[dapple](crates/dapple/README.md)** — CLI utilities for development and operations
- **[dapple-parser](crates/dapple-parser/README.md)** — Guide content parser (markdown directives, TOML metadata, Fluent messages)
- **[dapple-render](crates/dapple-render/README.md)** — Content rendering (markdown-to-HTML, PDF generation, tag-based filtering)

## Quick Start

### Prerequisites

- Rust 1.75+ (2024 edition)
- PostgreSQL 15+
- Docker (optional, for containerized development)

### Environment Setup

```bash
# Set database connection string
export DATABASE_URL="postgresql://user:pass@localhost:5432/dapple_dev"

# Optional: Enable SQLx offline mode for CI
export SQLX_OFFLINE=true
```

### Build and Run

```bash
# Build entire workspace
cargo build --workspace

# Run migrations
psql $DATABASE_URL -f crates/dapple-data/migrations/001_initial.sql

# Run API server
cargo run -p dapple-prompt-api

# Run tests
cargo test --workspace
```

## Core Concepts

### Data Model Principles

1. **Append-Only Architecture** — Mutations create new records instead of updating existing ones
2. **Batch Operations** — All operations use set-wise processing with `UNNEST WITH ORDINALITY`
3. **Versioned Objects** — Immutable versioning with path tracking for historical queries
4. **Type Safety** — SQLx compile-time query verification

### Object Types

- **Principal Objects** — Top-level entities (Playbook, Journey, Step, Task)
- **Owned Dependents** — Lifecycle tied to parent object
- **Shared Dependents** — Referenced across multiple principals
- **Versioned Objects** — Immutable versions with `(major, minor, patch)` semantics

### Model Taxonomy

Models are organized in a hierarchy that separates concerns:

```
Provider → Lineage → Tier → Release → Variant
```

- **Provider** — Where the model is hosted (Anthropic, self-hosted)
- **Lineage** — The "prompting language" a model family understands (claude-3-5, vits, flan-t5)
- **Tier** — Capability level within a lineage (sonnet, opus, haiku)
- **Release** — Version snapshot (20241022, v0.1)
- **InferenceProfile** — Journey-level model configuration

This architecture allows Playbooks to be model-agnostic. The same workflow can run on Claude, Mistral, or self-hosted models by changing the InferenceProfile attached to the Journey.

## Philosophy: Journey-Centric Design

Dapple Prompt follows a journey-centric design philosophy:

### What Users Experience

Users interact with **Journeys**, not internal objects. The flow:

1. **Pick a workflow** (Playbook)
2. **Complete an interview** (Debrief) to gather needed information
3. **Execute** (Dispatch) the workflow
4. **Review results** and iterate if needed

### Internal vs User-Facing

| Internal | User Experience |
|----------|-----------------|
| Playbook | Workflow to pick |
| Journey | User's session |
| Stage | A form / pane the user advances between |
| Step | A sub-interaction *on* a pane (generate → pick → refine) |
| Brief, BriefPack, BriefGroup | The debrief interview |
| Dispatch | Background execution |

### Stages are panes; steps are the interactions on them

A **Stage** is a static *form / pane* the user lands on; advancing to the next
stage is the real navigation to the next pane. **Steps** are the dynamic, AJAXy
sub-interactions *within* a pane (generate candidates → pick one → refine) before
the user navigates on. **Multi-step stages are the norm** — that's what steps are
for. Clients poll `GET /api/journeys/{id}/steps` for the fine-grained "which
sub-interaction is awaiting input" signal and `/stages` for the per-pane roll-up.
Value-slot overlays (`expose`/`mask`) are authored per node: a slot a specific
step needs or *produces* (a computed output) is exposed at the **step** level;
stage-level `expose` is for a slot the whole pane shares.

API consumers should hide internal terminology from end users. See the [Journey Planner's Guide - API Consumer Chapters](crates/dapple-prompt-api/docs/journey-planners-guide/14-authentication.md) for implementation guidance.

### API Design

REST endpoints follow a consistent pattern:
- `POST /resource` — Create batch of resources
- `GET /resource` — Query resources with filters
- `PATCH /resource` — Update (creates new version for versioned objects)

All operations preserve ordering and return results in the same order as inputs.

## Development Workflow

### Running Tests

```bash
# Integration tests (requires DATABASE_URL)
cargo test --package dapple-prompt-api --test user-lifecycle

# Unit tests
cargo test --package dapple-data --lib

# With logging
RUST_LOG=debug cargo test
```

### Code Quality

```bash
# Format code
cargo fmt --all

# Lint
cargo clippy --workspace -- -D warnings

# Prepare SQLx metadata (preferred: `dapple sqlx prepare`, which adds
# `--all-targets` so test-target inline queries are cached too)
cargo sqlx prepare --workspace -- --all-targets
```

### Database Migrations

Migrations live in `crates/dapple-data/migrations/` and should be applied sequentially:

```bash
# Apply all migrations
for f in crates/dapple-data/migrations/*.sql; do
    psql $DATABASE_URL -f "$f"
done
```

## Project Structure

```
dapple-prompt/
├── crates/
│   ├── dapple-data/
│   │   ├── assets/sql/         # SQL query files
│   │   ├── migrations/         # Database migrations
│   │   └── src/
│   │       ├── model/          # Rust models
│   │       └── query/          # Query functions
│   ├── dapple-prompt-api/
│   │   ├── src/
│   │   │   ├── handler/        # Axum handlers
│   │   │   ├── route/          # Route definitions
│   │   │   └── schema/         # Request/response schemas
│   │   └── tests/              # Integration tests
│   ├── dapple-prompt-conduit/
│   │   └── src/
│   │       ├── handler/        # Prompt execution endpoints
│   │       └── service/        # LLM backends, templates, embeddings
│   ├── dapple-prompt-flow/
│   │   └── src/                # Workflow engine
│   ├── dapple-parser/
│   │   └── src/
│   │       ├── lib.rs             # Guide parser entry point
│   │       └── transform/         # Pure transform layer
│   └── dapple-render/
│       └── src/                   # Content rendering
├── docs/
│   ├── changes/                # Completed implementation plans (changelog)
│   ├── plans/                  # Pending/future work plans
│   └── musings/                # Deferred project ideas
├── CLAUDE.md                   # Comprehensive development guide
├── AGENTS.md                   # Agent-specific instructions
└── Cargo.toml                  # Workspace definition
```

## Documentation

- **[CLAUDE.md](CLAUDE.md)** — Comprehensive development guide and patterns
- **[AGENTS.md](AGENTS.md)** — Instructions for LLM agents
- **[Journey Planner's Guide](crates/dapple-prompt-api/docs/journey-planners-guide/SUMMARY.md)** — Complete guide for playbook authoring and API consumption
- **[dapple-data README](crates/dapple-data/README.md)** — Data layer documentation
- **[dapple-prompt-api README](crates/dapple-prompt-api/README.md)** — API service documentation
- **[Redis Caching](assets/docs/redis-object-caching.md)** — Caching strategy

## Key Features

- **OpenAPI 3.1 Documentation** — Automatic API docs via `aide`
- **Batch-First Design** — All operations optimized for bulk processing
- **Redis Caching** — Configurable object caching layer
- **Nushell Workflows** — Secure, sandboxed task execution
- **Type-Safe Queries** — Compile-time SQL verification with SQLx
- **Multi-Backend LLM** — Anthropic Claude, local Mistral (via Candle), extensible
- **Local Embeddings** — ModernBERT embeddings via Candle for semantic context selection
- **Guide Ingest Pipeline** — Tarball-based guide ingestion with atomic transaction and dry-run support
- **Fluent-Native Content** — Locale-independent guide structure with Fluent (.ftl) message files
- **Content Rendering** — Markdown rendering with tag-based filtering for audience-specific output

## Contributing

Issues and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md); every commit needs a DCO sign-off (`git commit -s`). Please report security issues privately as described in [SECURITY.md](SECURITY.md), and follow our [Code of Conduct](CODE_OF_CONDUCT.md).

The codebase follows strict conventions documented in [CLAUDE.md](CLAUDE.md) and each crate's `AGENTS.md`:

1. All SQL lives in `.sqlc` source files under `crates/dapple-data/assets/sqlc/`; the generated `assets/.sql/` is never edited by hand
2. No inline SQL in Rust code
3. Batch operations must preserve ordinality
4. Integration tests verify POST/GET consistency

## Environment Variables

| Variable | Description | Required |
|----------|-------------|----------|
| `DATABASE_URL` | PostgreSQL connection string | Yes |
| `SQLX_OFFLINE` | Enable offline query verification | No |
| `RUST_LOG` | Logging level (`debug`, `info`, `warn`) | No |
| `REDIS_URL` | Redis connection string | No |
| `ANTHROPIC_API_KEY` | Anthropic API key for Claude models | No |
| `MISTRAL_ENABLED` | Enable local Mistral inference (`true`/`false`) | No |
| `MISTRAL_DEVICE` | Inference device: `cpu`, `cuda:0`, `metal` | No |
| `HF_TOKEN` | HuggingFace token for gated models | No |

## License

Dapple Prompt is licensed under the [Apache License, Version 2.0](LICENSE) (`Apache-2.0`).
Copyright 2025-2026 DappleAI Co. Third-party notices are in [NOTICE](NOTICE).

"Dapple", "DappleAI" and the Dapple logos are trademarks of DappleAI Co. and are not covered by the Apache license; see [TRADEMARK.md](TRADEMARK.md).

## Support

For issues or questions, refer to the comprehensive guide in [CLAUDE.md](CLAUDE.md) or the crate-specific documentation.
