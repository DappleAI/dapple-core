# Preface

<div style="text-align: center; margin: 1em 0 2em 0;">
<strong style="font-size: 1.2em;">Version 0.2.1</strong><br/>
<a href="./CHANGES.md">What's new in this version?</a>
</div>

Welcome to the **Journey Builder Guide** for dapple-prompt.

## Guide Version
0.2.1

## Who This Guide Is For

This guide is designed for:

- **Backend Developers** working on dapple-prompt core services
- **Platform Engineers** deploying and extending the system
- **Guide Authors** who need to understand engineering internals for debugging ingest issues
- **Contributors** who want to understand the architecture
- **AI Agents** that need deep system knowledge for code generation or analysis

## What You'll Learn

- **Guide system internals** — how `dapple-parser` parses guide directories, how the transform layer produces `IngestPlan`, and how the API handler materializes guides to the database
- **SDK architecture** — how `dapple-sdk` is structured, how code generation works, and how to add new resource clients
- Architecture of the dapple-prompt platform
- How the Flow Engine (dapple-prompt-flow) executes workflows
- How the Conduit Service routes prompts to LLM backends
- Render pipeline for tag-filtered PDF generation
- Custom Nushell command development
- Data layer patterns and conventions
- Testing and debugging techniques

## Prerequisites

- Proficiency in Rust programming
- Familiarity with async programming and Tokio
- Understanding of PostgreSQL and SQL
- Basic knowledge of Nushell scripting
- Experience with REST API development

## How to Use This Guide

1. Start with [Architecture Overview](./getting-started/architecture-overview.md) for the big picture
2. Read [Guide System Internals](./guide-system/parser-architecture.md) to understand the ingest pipeline
3. Study [SDK Architecture](./sdk/overview.md) for the typed client internals
4. Dive into [Flow Engine](./flow/overview.md) for workflow execution
5. Study [Conduit Service](./conduit/overview.md) for LLM routing
6. Reference [Data Layer](./data-layer/query-patterns.md) for database patterns
7. Use [Development](./development/local-development.md) for setup and testing

> **For guide authoring tutorials**, see the **Journey Planner Guide**, which covers writing guides, SDK usage from the author perspective, directives, Fluent localization, and the ingest pipeline from the authoring side.

## Key Crates

| Crate | Purpose |
|-------|---------|
| `dapple-prompt-api` | REST API service (Axum handlers, routes, guide ingest) |
| `dapple-prompt-flow` | Workflow engine (Nushell integration, dataflow-rs) |
| `dapple-prompt-conduit` | LLM routing service (Handlebars, multi-backend) |
| `dapple-data` | Data access layer (PostgreSQL, SQLx, models) |
| `dapple-parser` | Guide content parser (directives, TOML, Fluent, SUMMARY.md) |
| `dapple-render` | PDF rendering (tag filtering, branding styles) |
| `dapple-sdk` | Typed Rust SDK (auto-generated from OpenAPI, three auth methods) |
| `dapple-fluent` | Fluent template parsing, rendering, and modeling |

## Related Guides

- **API Consumer Guide** — For consuming the REST API
- **Journey Planner Guide** — For designing playbooks, authoring guides, and SDK usage tutorials

## Feedback

If you find errors or have suggestions, please open an issue on GitHub.
