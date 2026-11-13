# Introduction

> **Audience:** Backend Developers · Platform Engineers · Contributors

Welcome to the dapple-prompt developer documentation. This guide covers the internal architecture and implementation patterns for working on the platform.

> **Relationship to the Journey Planner Guide:** The Journey Planner Guide covers guide authoring, SDK usage, and the ingest pipeline from the *author/planner perspective*. This guide covers *engineering internals*: how to extend the parser, how materialization works in the data layer, how to add new directive types, SDK architecture for extension, and render pipeline internals.

## Guide-Centric Architecture

Guides are the primary authoring unit in dapple-prompt. All backend objects — playbooks, tasks, inference profiles, brief packs — are **derived from guides** through the ingest pipeline. Authors write structured markdown with directives; the platform parses, transforms, and materializes these into database entities.

```
Guide Directory → dapple-parser → IngestPlan → dapple-prompt-api → PostgreSQL
                                                                        ↓
                                                                   dapple-sdk
```

The SDK (`dapple-sdk`) is the primary programmatic interface for querying and managing these objects after ingest.

## Platform Overview

dapple-prompt is built as a monorepo with multiple Rust crates:

```
crates/
├── dapple-prompt-api     # REST API service (Axum)
├── dapple-prompt-flow    # Workflow engine (Nushell + dataflow-rs)
├── dapple-prompt-conduit # LLM routing (Handlebars templates)
├── dapple-data           # Data access layer (PostgreSQL + SQLx)
├── dapple-parser         # Guide content parser (directives, TOML, Fluent, SUMMARY.md)
├── dapple-render         # PDF rendering with tag filtering and branding
├── dapple-fluent         # Fluent template parsing, rendering, and modeling
├── dapple-sdk            # Typed Rust SDK (auto-generated from OpenAPI)
└── dapple                # CLI utilities
```

## Core Services

### dapple-prompt-api

The REST API service built with Axum. Handles:
- HTTP request routing and guide ingest (multipart tarball upload)
- Authentication and authorization
- Request validation (schemars)
- Guide materialization (IngestPlan → database)

### dapple-parser

The guide parsing and transformation crate. Provides:
- Fence tokenizer for `:::directive { attrs }` syntax
- Block classifier mapping tokens to typed blocks
- Transform layer producing `IngestPlan` from `ParsedGuide` (pure, no DB access)
- Fluent `.ftl` file discovery and parsing

### dapple-render

The PDF rendering crate. Provides:
- Tag-based block filtering by audience
- Render profiles with branding styles (consumer, technical, internal)
- Cover page, table of contents, and chapter rendering

### dapple-sdk

The typed Rust SDK. Provides:
- Auto-generated types from OpenAPI spec (via typify)
- Three auth methods: API token, Bearer JWT, Ed25519 service token
- Resource clients for all API endpoints
- Non-standard endpoints: multipart ingest, binary PDF render, streaming assets

### dapple-prompt-flow

The workflow execution engine. Features:
- Nushell script execution in sandboxed environment
- Step-by-step task orchestration
- Directive system for conditional execution, retry, and branching
- Integration with dataflow-rs

### dapple-prompt-conduit

The LLM routing service. Provides:
- Multi-backend support (Anthropic, Mistral, local)
- Handlebars template rendering
- Model resolution via inference profiles
- Overlay configuration system

### dapple-data

The data access layer. Implements:
- SQLx compile-time verified queries
- PostgreSQL with append-only patterns
- Versioned object model
- Batch operations with UNNEST

## Data Flow

### Guide Ingest (Primary Creation Path)

```
Author writes guide directory
     ↓
POST /api/guides/ingest (tarball)
     ↓
dapple-parser::parse_guide() → ParsedGuide
     ↓
dapple-parser::transform::plan_guide() → IngestPlan
     ↓
materialize_guide() (single transaction)
     ↓
PostgreSQL (guide + chapters + blocks + derived objects)
```

### API Request (Query Path)

```
SDK / HTTP Client
     ↓
dapple-prompt-api (Axum handler)
     ↓
dapple-data (Query function)
     ↓
Generated SQL (assets/.sql/*)
     ↓
PostgreSQL
     ↓
Response DTO
```

## Development Setup

```bash
# Required
export DAPPLE_PROMPT_DATABASE_URL="postgresql://user:pass@localhost/dapple_dev"

# Run tests
cargo test --workspace

# Run API server
cargo run -p dapple-prompt-api
```

## Next Steps

- [Architecture Overview](./architecture-overview.md) — System component diagram and guide ingest pipeline
- [Glossary](./glossary.md) — Platform terminology
- [Core Concepts](./concepts.md) — Data model fundamentals
- [Guide System Internals](../guide-system/parser-architecture.md) — Parser architecture deep dive
- [SDK Architecture](../sdk/overview.md) — SDK internals for extension
- [Flow Engine](../flow/overview.md) — Workflow execution details
