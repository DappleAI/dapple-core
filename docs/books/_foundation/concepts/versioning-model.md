# Versioning Model

> **Audience:** Prompt Engineers · Product · Developers · Agents

This document describes how versioning works in dapple-prompt for objects, prompts, and packs.

---

<!-- ANCHOR: overview -->
## Overview

dapple-prompt uses an append-only versioning model. When you update a versioned object, a new version is created rather than modifying the existing one. This provides:

- **Audit trail:** Complete history of all changes
- **Rollback capability:** Reference any previous version
- **Concurrent safety:** No data races on updates
<!-- ANCHOR_END: overview -->

---

<!-- ANCHOR: versioned-objects -->
## Versioned Objects

Versioned objects in dapple-prompt include:

| Object | Description |
|--------|-------------|
| ValueSlot | Typed value definitions |
| Brief | Debrief UI containers |
| BriefGroup | Atomic brief bundles |
| BriefPack | Distributable brief packages |
| FluentMessage | Localized text |
| Prompt | LLM prompt templates |
| Asset | Binary files (images, documents) |
| Directive | JSONLogic conditional execution rules |
| Guide | Structured documentation (chapters, blocks, tags) |

**Key identifiers:**

| Field | Description |
|-------|-------------|
| `object_id` | UUID for this specific version |
| `path_object_id` | UUID for the version lineage (stable across versions) |
| `version` | `{ major, minor, patch }` triplet |
<!-- ANCHOR_END: versioned-objects -->

---

<!-- ANCHOR: version-lifecycle -->
## Version Lifecycle

**Creating (POST):**
- Creates a new path (lineage) and first version
- Version starts at `0.0.1`
- Returns both `object_id` and `path_object_id`

**Updating (PATCH):**
- Creates a new version under the existing path
- Increments the patch version by default
- Previous versions remain accessible
- `path_object_id` remains stable

```json
// POST creates version 0.0.1
POST /api/briefs/
{ "objs": [{ "name": "contact-form", "title": "Contact Info" }] }

// Response
{
  "object_id": "version-1-uuid",
  "path_object_id": "path-uuid",
  "version": { "major": 0, "minor": 0, "patch": 1 }
}

// PATCH creates version 0.0.2
PATCH /api/briefs/
{ "objs": [{ "path_object_id": "path-uuid", "title": "Contact Information" }] }

// Response
{
  "object_id": "version-2-uuid",
  "path_object_id": "path-uuid",
  "version": { "major": 0, "minor": 0, "patch": 2 }
}
```
<!-- ANCHOR_END: version-lifecycle -->

---

<!-- ANCHOR: querying-versions -->
## Querying Versions

**Get specific version:**
```
GET /api/briefs/?query=by_object_ids&ids=["version-1-uuid"]
```

**Get all versions of a path:**
```
GET /api/briefs/?query=by_path_ids&path_ids=["path-uuid"]
```

**Get latest version:**
The API returns versions ordered by creation. The last item is typically the latest, but you can also query paths directly:
```
GET /api/brief-paths/?query=by_object_ids&ids=["path-uuid"]
```
<!-- ANCHOR_END: querying-versions -->

---

<!-- ANCHOR: naming-conventions -->
## Naming Conventions

Use consistent naming patterns across the platform:

### Prompts

Use dotted version suffixes:
```
resume-data-extractor.v1
cover-letter-generator.v2
```

### Packs

Use semantic versioning:
```
brief-pack-0.1.0
resume-builder-1.2.3
```

### Tasks, Steps, Stages, Playbooks

Use short kebab-case identifiers:

| Type | Example |
|------|---------|
| Task | `format-resume`, `render-pdf` |
| Step | `build-resume-step` |
| Stage | `stage-1`, `data-collection` |
| Playbook | `build-resume-playbook` |
<!-- ANCHOR_END: naming-conventions -->

---

<!-- ANCHOR: best-practices -->
## Best Practices

1. **Reference by path for "latest":** When you want the current version of something, reference the `path_object_id` and resolve to the latest version at runtime.

2. **Pin versions for reproducibility:** When you need deterministic behavior (e.g., in production), reference specific `object_id` values.

3. **Use semantic versioning for breaking changes:**
   - Patch (`0.0.x`): Bug fixes, minor tweaks
   - Minor (`0.x.0`): New features, backward compatible
   - Major (`x.0.0`): Breaking changes

4. **Archive incompatible versions:** Don't delete old versions; keep them for audit and rollback.

5. **Document version changes:** Maintain a changelog for significant version updates.
<!-- ANCHOR_END: best-practices -->
