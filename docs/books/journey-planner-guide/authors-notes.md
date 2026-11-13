# Authors Notes & WIP Features

> **Audience:** Prompt Engineers · Product · Agents

This section describes work-in-progress platform features and author-facing assumptions that may change.

---

## Resolved Features

### Guide Content Parsing
**Status:** Resolved (v0.3.0)

The automated guide-to-entity parsing pipeline is fully implemented. Authors write `guide.toml` + markdown chapters with `:::directive` blocks + Fluent `.ftl` files, package as a tarball, and ingest via `POST /api/guides/ingest` or the Dapple SDK's `client.guides().ingest()`. The pipeline atomically creates all derived objects.

See: [Guide Directory Structure](./guide-directory-structure.md), [Ingest Pipeline](./ingest-pipeline.md)

### Guide PATCH Endpoint
**Status:** Resolved (v0.3.0)

Guide PATCH ingest is fully implemented via `PATCH /api/guides/{path_object_id}/ingest`. Creates a new version under the existing path while keeping the slug stable.

See: [Versioning](./versioning.md#guide-versioning), [Ingest Pipeline](./ingest-pipeline.md#post-vs-patch)

---

## Partially Resolved Features

### Prompt Registry
**Status:** Partially Resolved

Prompts are now defined as `:::inference_profile` directives within guides, with Fluent attributes carrying system prompts, temperature, and max tokens. The inference profile system handles model-to-prompt binding via `rev.inference_profile_configs`. A standalone prompt registry with independent versioning and A/B testing is not yet implemented.

See: [Defining Inference Profiles](./defining-inference-profiles.md)

### Versioned Brief Pack Publishing
**Status:** Partially Resolved

Brief packs can now be declared as `:::brief_pack` directives within guides and are extracted during ingest. The full Brief/BriefGroup/BriefPack hierarchy with Fluent attribute mapping is still being wired. A standalone pack registry with dependency resolution is not yet available.

See: [Directive Blocks](./directive-blocks.md)

---

## WIP Features

### Overlay Management UI
**Status:** Planned

A UI for configuring overlays (runtime model routing overrides) without editing code.

**Current workaround:** Configure overlays programmatically via the API.

### Dispatch Interrupt API
**Status:** Planned

Mid-execution return to debrief — allowing a running dispatch to pause, collect additional user input, and resume.

**Current workaround:** Design workflows with explicit review stages rather than mid-execution interrupts.

---

## Assumed Contracts

These conventions are expected to remain stable:

| Contract | Status | Notes |
|----------|--------|-------|
| `{ payload, signals }` task return | Stable | Core Nushell task contract |
| `guide.toml` format | Stable | `title` required, semver version, `[card]`, `[deprecation]` |
| `:::directive { attrs }` syntax | Stable | 10 known directive types |
| `locales/*.ftl` convention | Stable | Kebab-case keys, Fluent attributes |
| `data.*` / `metadata.*` namespaces | Stable | Canonical payloads vs control-plane |
| `artifact.json` schema | Stable | `{path, type}` minimum |
| `OUT_DIR` / `IN_ARTIFACT` env vars | Stable | Core runtime contract |

---

## Known Gaps

- **Dispatch interrupt API** — Mid-execution return to debrief not yet implemented
- **Brief pack full materialization** — `:::brief_pack` directive extraction works but full Brief/BriefGroup hierarchy wiring is in progress
- **Shared content scope enforcement** — Scope visibility rules are documented but enforcement during ingest is partial
- **Multi-locale testing** — Localization pipeline works but automated testing across multiple locales is limited

---

## Feedback

If you encounter issues or have suggestions:
- File issues in the repository
- Tag with `journey-planner-guide` for documentation feedback
- Tag with `wip-feature` for feature requests
