# Changes

> **:robot: Agent Note:** Skip this chapter. Changes documentation is for human readers tracking version-to-version differences. Agents should read the actual content chapters directly.

This chapter helps human readers understand what changed between versions so you don't have to reread the entire guide. Each version links to the new or updated sections.

---

## [Version 0.2.1](#version-021) (Current)

**Released:** 2026-02-19

This release documents the Guide Lifecycle Audit platform changes.

### What Changed

**Materialization Pipeline updated ([materialization.md](./guide-system/materialization.md)):**
- Full 6-step derived object pipeline documented with expanded mermaid diagram
- Prompt entity creation, task-prompt cross-product, and inference profile config wiring added
- Name-based dedup for inference profiles and prompts

**Model Taxonomy updated ([model-taxonomy.md](./conduit/model-taxonomy.md)):**
- Claude 4.6 lineage (`claude-4-6/sonnet/20250929`) with release chain example
- Lineage-aware `map_model(lineage_key, tier)` documented with code snippet
- "Adding a New Lineage" checklist includes backend routing update step

**LLM Backends updated ([llm-backends.md](./conduit/llm-backends.md)):**
- Claude 4.6 models added to supported list
- Lineage-aware routing note added to Model Configuration section

### Quick Links for 0.2.1

If upgrading from 0.2.0:
1. [Materialization - Derived Objects](./guide-system/materialization.md#6-derived-object-materialization) -- Full pipeline
2. [Model Taxonomy - Anthropic Backend](./conduit/model-taxonomy.md#anthropic-backend) -- Lineage-aware routing
3. [LLM Backends](./conduit/llm-backends.md#supported-models) -- Updated model list

---

## [Version 0.2.0](#version-020)

**Released:** 2026-02-18

This release makes the guide **guide-centric**, reflecting the architectural evolution where guides are the primary authoring surface for all platform objects.

### What's New

**Guide System Internals (6 new chapters):**
- [Parser Architecture](./guide-system/parser-architecture.md) — How `dapple-parser` tokenizes, classifies, and parses guide directories
- [Transform Layer](./guide-system/transform-layer.md) — Pure `plan_guide()` transform from `ParsedGuide` to `IngestPlan`
- [Materialization Pipeline](./guide-system/materialization.md) — How the API handler materializes IngestPlan to database in a single transaction
- [Guide Data Model](./guide-system/data-model.md) — Database schema, SQL patterns, Rust models, and fold functions
- [Render Pipeline](./guide-system/render-pipeline.md) — Tag-filtered PDF generation with branding styles
- [Extending the Guide System](./guide-system/extending.md) — Step-by-step checklists for adding directives, tags, profiles, and scopes

**SDK Architecture (2 new chapters):**
- [SDK Overview](./sdk/overview.md) — Crate structure, code generation, auth methods, non-standard endpoints
- [Adding Resource Clients](./sdk/adding-resource-clients.md) — Step-by-step guide for extending the SDK

**Getting Started updated:**
- [Introduction](./getting-started/intro.md) — Guide-centric architecture explanation, expanded crate tree
- [Architecture Overview](./getting-started/architecture-overview.md) — Guide ingest pipeline diagram, parser/render/SDK crate docs, fixed stale `maker_key` reference
- [Core Concepts](./getting-started/concepts.md) — New Guide System and SDK concept sections

**Existing sections updated:**
- [Conduit Overview](./conduit/overview.md) — Added inference profile guide integration context
- [Testing Patterns](./development/testing-patterns.md) — Added SDK-based testing examples and dry-run validation
- [Preface](./preface.md) — Version 0.2.0, expanded audience and learning objectives
- [AGENTS.md](./AGENTS.md) — Full crate structure, guide system navigation

**Foundation glossary expanded:**
- Updated Guide definition to emphasize primary authoring unit
- Added engineering terms: `IngestPlan`, `ParsedGuide`, `FluentMessageSet`, `SharedContentPlan`, `Materialization`
- Added SDK terms: `DappleClient`, `Resource Client`, `Render Profile`
- Added crate terms: `dapple-parser`, `dapple-render`, `dapple-sdk`

### Breaking Changes

No breaking platform changes — this release is documentation-only.

### Stale References Fixed

- `maker_key` in EngineConfig → `lineage_key` in ModelConfig (architecture-overview.md line 80)

### Quick Links for 0.2.0

1. [Guide System: Parser Architecture](./guide-system/parser-architecture.md) — Start here for ingest internals
2. [SDK: Overview](./sdk/overview.md) — SDK structure and extension
3. [Architecture Overview](./getting-started/architecture-overview.md) — Updated system diagram

---

## [Version 0.1.0](#version-010)

**Released:** 2026-01-31

This is the first versioned release of the Journey Builder Guide. It introduces comprehensive documentation for the model taxonomy architecture.

### What's New

**Model Taxonomy introduced:**
- New `ModelConfig` and `ModelService` replacing legacy `EngineConfig` and `EngineService`
- Lineage-based routing for specialized handlers (TTS, summarization, MLM)
- Release chain format for template resolution
- See: [Conduit: Model Taxonomy](./conduit/model-taxonomy.md)

**Conduit documentation updated:**
- Overview updated with `ModelService` architecture
- Mermaid diagrams reflect new service structure
- Request flow shows model resolution
- See: [Conduit Overview](./conduit/overview.md)

**Database schema changes documented:**
- New tables: `providers`, `lineages`, `model_releases`, `model_variants`
- InferenceProfile tables for journey-level model configuration
- Legacy tables (`llm_engines`, `llm_engine_makers`, `playbook_step_engine_configs`) removed
- See: [Conduit: Model Taxonomy - Database Schema](./conduit/model-taxonomy.md#database-schema)

**Backend routing updated:**
- `LlmBackend` trait now uses `ModelConfig` instead of `EngineConfig`
- `supports()` method checks `lineage_key` and `provider_key`
- See: [Conduit: Model Taxonomy - Backend Routing](./conduit/model-taxonomy.md#backend-routing)

### Breaking Changes

This version documents breaking changes to the platform:
- `EngineService` removed (use `ModelService`)
- `EngineConfig` removed (use `ModelConfig`)
- `playbook_step_engine_configs` table removed
- Routing uses `lineage_key` instead of `maker_key`

### Quick Links for 0.1.0

1. [Conduit Model Taxonomy](./conduit/model-taxonomy.md) - Full technical documentation
2. [Conduit Overview](./conduit/overview.md) - Updated architecture diagrams
3. [Foundation Model Taxonomy](../_foundation/concepts/model-taxonomy.md) - Shared conceptual doc

---

## Future Versions

### Planned for 0.3.0

- [ ] Directive system documentation (flow control, retry, branching)
- [ ] Asset streaming endpoint internals
- [ ] Advanced backend configuration and provider fallback strategies
- [ ] Variant management and fine-tuning workflows
