# Appendix — File Map & Reference

> **Audience:** :wrench: Prompt Engineers · :robot: Agents

Quick reference for codebase locations and key concepts.

---

## File Map

### Data Layer (`dapple-data`)

| Location | Contents |
|----------|----------|
| `crates/dapple-data/migrations/` | Database migrations |
| `crates/dapple-data/assets/sql/**` | SQL query files |
| `crates/dapple-data/src/model/**` | Rust model definitions |
| `crates/dapple-data/src/query/**` | Query functions |

### API Layer (`dapple-prompt-api`)

| Location | Contents |
|----------|----------|
| `crates/dapple-prompt-api/src/handler/**` | HTTP handlers |
| `crates/dapple-prompt-api/src/schema/**` | Request/response schemas |
| `crates/dapple-prompt-api/src/route/**` | Route definitions |
| `crates/dapple-prompt-api/tests/` | Integration tests |

### Guide Data

| Location | Contents |
|----------|----------|
| `crates/dapple-data/assets/sql/guides/**` | Guide SQL queries |
| `crates/dapple-data/src/model/guides.rs` | Guide model definitions |
| `crates/dapple-data/src/query/guides.rs` | Guide query functions |
| `crates/dapple-prompt-api/src/handler/guides.rs` | Guide API handlers |
| `crates/dapple-prompt-api/src/schema/guides.rs` | Guide request/response schemas |

### Runtime (`dapple-prompt-flow`, `dapple-prompt-conduit`)

| Location | Contents |
|----------|----------|
| `crates/dapple-prompt-flow/` | Workflow execution engine |
| `crates/dapple-prompt-conduit/` | LLM routing service |

---

## Guide Directory File Reference

| File/Directory | Required | Description |
|----------------|----------|-------------|
| `guide.toml` | Yes | Guide metadata (title, description, version, card, deprecation) |
| `SUMMARY.md` | Yes | Chapter ordering and hierarchy |
| `src/*.md` (or `*.md`) | Yes | Chapter content with directive blocks |
| `locales/*.ftl` | No | Fluent localization files (one per locale) |
| `assets/` | No | Binary assets (images, documents) |

### Directive Block Types

| Type | Content Source | Creates |
|------|--------------|---------|
| `prose` | Fluent | -- (narrative content) |
| `stage` | Fluent | Stage in derived playbook |
| `step` | Fluent | Step in derived playbook |
| `nushell` | Inline | Task entity |
| `brief_pack` | Fluent | Brief pack entity |
| `inference_profile` | Fluent | Inference profile entity |
| `ref` | Inline | Shared content reference |
| `block` | Fluent | Generic content block |

---

## Quick Glossary

| Term | Definition |
|------|------------|
| **Journey** | Runtime instance of a Playbook assigned to a user |
| **Playbook** | Authored workflow composed of Stages, Steps, and Tasks |
| **Stage** | Major phase in a Playbook (e.g., data collection, review) |
| **Step** | Ordered group of Tasks within a Stage |
| **Task** | Atomic unit of work (script or program) |
| **Brief** | Versioned container for debrief UI elements |
| **BriefGroup** | Atomic grouping of Briefs (must complete together) |
| **BriefPack** | Versioned bundle of BriefGroups assigned to Journeys |
| **Dispatch** | Runtime execution of a Playbook |
| **Debrief** | User interview to collect input data |
| **Guide** | Versioned structured documentation entity (chapters, blocks, tags) |
| **Guide Directory** | On-disk structure containing `guide.toml`, `SUMMARY.md`, chapters, `locales/`, and `assets/` |
| **guide.toml** | Metadata file at the root of a guide directory defining title, description, version, card, and deprecation |
| **GuideChapter** | Ordered section within a Guide, can be nested |
| **GuideBlock** | Content unit with directive_type, content, attributes, and tags |
| **Directive Block** | A fenced content block in guide markdown (`:::type { attrs } ... :::`) that maps to a platform entity |
| **IngestPlan** | The pure transform output from `plan_guide()` containing all derived objects and metadata |
| **Dry Run** | Validation-only ingest mode (`dry_run=true`) that returns parsed results without writing to the database |
| **Dapple SDK** | Typed Rust HTTP client (`dapple-sdk` crate) providing ergonomic access to all API resources |

---

## Key Schemas

### artifact.json

```json
{
  "path": "./out/resume.pdf",
  "type": "application/pdf"
}
```

### error.json

```json
{
  "message": "Failed to render PDF",
  "code": 500
}
```

### checkpoint.json

```json
{
  "approved": true,
  "notes": "Looks good"
}
```

---

## Environment Variables

| Variable | Description |
|----------|-------------|
| `OUT_DIR` | Output directory for artifacts |
| `IN_ARTIFACT` | Path to previous Task's artifact |
| `PLAYBOOK_ID` | Current Playbook identifier |
| `STAGE_ID` | Current Stage identifier |
| `STEP_ID` | Current Step identifier |
| `TASK_ID` | Current Task identifier |
| `PROMPT_ID` | Prompt template identifier |
| `CONDUIT_OPTIONS` | JSON blob for provider config |

---

## Known Gaps & Roadmap

See `AGENTS.md` for documented API gaps:

- BriefGroup fetch endpoint
- Batch fetch for BriefPacks
- Dispatch interrupt API
- Journey state transitions

---

## Related Documentation

- [Guide Directory Structure](./guide-directory-structure.md) -- Anatomy of a guide directory
- [Ingest Pipeline](./ingest-pipeline.md) -- How guide tarballs are parsed, transformed, and materialized
- [Directives and JSONLogic](./directives.md) -- Flow control with JSONLogic
- [Task Execution Runtime](./dapple-prompt-flow.md) -- Nushell task execution
- [Prompt Execution and Routing](./dapple-prompt-conduit.md) -- LLM routing layer
- **API Consumer Guide:** [SDK Overview](../api-consumer-guide/sdk-overview.md), [API Patterns](../api-consumer-guide/api-patterns.md), [API Cookbook](../api-consumer-guide/api-cookbook.md)
- [CLAUDE.md](../../../../CLAUDE.md) -- Development guidelines
- [AGENTS.md](../../../../AGENTS.md) -- Agent-specific guidance
