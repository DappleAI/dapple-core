# Core Concepts

> **Audience:** Backend Developers · Platform Engineers

This chapter covers the foundational concepts for understanding the dapple-prompt platform architecture.

## Execution Model

{{#include ../../_foundation/concepts/core-execution-model.md}}

---

## Briefs & Packs

{{#include ../../_foundation/concepts/briefs-and-packs.md}}

---

## Artifacts & Files

{{#include ../../_foundation/concepts/artifacts-and-files.md}}

---

## Environment Variables

{{#include ../../_foundation/concepts/environment-variables.md}}

---

## Versioning Model

{{#include ../../_foundation/concepts/versioning-model.md}}

---

## Guide System

Guides are the primary authoring unit — all backend objects (playbooks, tasks, inference profiles) are derived from guides through the ingest pipeline.

| Concept | Description |
|---------|-------------|
| **ParsedGuide** | AST from parsing a guide directory (chapters, blocks, metadata, locales) |
| **IngestPlan** | Transform output — everything needed to materialize a guide |
| **FluentMessageSet** | Localized message bodies grouped by key across locales |
| **Materialization** | Persisting an IngestPlan to the database in a single transaction |
| **Derived Objects** | Playbooks, tasks, brief packs, inference profiles extracted from directives |

For full details, see [Guide System Internals](../guide-system/parser-architecture.md).

---

## SDK

The `dapple-sdk` crate provides a typed Rust client for the API, auto-generated from the OpenAPI spec.

| Concept | Description |
|---------|-------------|
| **DappleClient** | Main client struct with resource accessor methods |
| **Resource Client** | Typed sub-client (e.g., `GuidesClient`, `PlaybooksClient`) for each endpoint |
| **AuthProvider** | Trait for pluggable authentication (API token, JWT, Ed25519) |

For full details, see [SDK Architecture](../sdk/overview.md).
