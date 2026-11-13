# Briefs & Packs

> **Audience:** Prompt Engineers · Product · Agents · Developers

This document describes the Brief system used for debrief interviews and user data collection.

---

<!-- ANCHOR: overview -->
## Overview

| Concept | Description |
|---------|-------------|
| **Brief** | A versioned container for UI elements (inputs, outputs, markdown) used in debrief interviews |
| **BriefGroup** | An atomic collection of related Briefs that must be completed together |
| **Brief Pack** | A distributable package containing BriefGroups, assigned to Journeys at creation time |
| **Pack Manifest** | The manifest file (YAML/JSON) at the root of a Brief Pack describing contents and metadata |
<!-- ANCHOR_END: overview -->

---

<!-- ANCHOR: hierarchy -->
## Hierarchy

```
Brief Pack
  └── BriefGroup (atomic)
        └── Brief (versioned)
              └── BriefUnit (UI element: input, output, markdown, notice)
```

**Key relationships:**
- A Journey is assigned one or more Brief Packs at creation time
- Brief Packs contain BriefGroups
- BriefGroups contain Briefs (via `brief_group_briefs` associative table)
- Briefs contain BriefUnits (owned dependents)
<!-- ANCHOR_END: hierarchy -->

---

<!-- ANCHOR: brief -->
## Brief

A Brief is a versioned container for UI elements displayed during a debrief interview. Each Brief:

- Has a unique `id` and `path_id` (for version tracking)
- Contains one or more BriefUnits
- Is referenced by BriefGroups

**Versioning:** Briefs use the standard versioned object pattern. PATCH creates a new version; the `path_id` remains stable across versions.
<!-- ANCHOR_END: brief -->

---

<!-- ANCHOR: brief-unit -->
## BriefUnit

BriefUnits are the individual UI elements within a Brief:

| Type | Description |
|------|-------------|
| `input` | User input field (text, textarea, select, radio, checkbox, etc.) |
| `output` | Display computed or retrieved data to the user |
| `markdown` | Static markdown content for instructions or context |
| `notice` | Alert or notification (info, warning, error) |

**Input controls:** text, textarea, select, radio, checkbox, date, number, file, toggle, color, range
<!-- ANCHOR_END: brief-unit -->

---

<!-- ANCHOR: brief-group -->
## BriefGroup

A BriefGroup is an atomic collection of related Briefs. **Atomicity is enforced:** users must complete all Briefs in a BriefGroup together.

**Design implications:**
- Your UI should present all BriefUnits from a single BriefGroup as one form section/screen
- Partial completion is not allowed. Either all answers are submitted or none.
- Group related questions that logically belong together

A debrief form-screen (a BriefGroup) is distinct from a playbook **step** — a step is a generate→pick→refine sub-interaction within a stage, surfaced via `GET /api/journeys/{id}/steps`.

**Example:** A "Contact Information" BriefGroup might contain Briefs for name, email, and phone, all required together.
<!-- ANCHOR_END: brief-group -->

---

<!-- ANCHOR: brief-pack -->
## Brief Pack

A Brief Pack is a distributable package of BriefGroups assigned to Journeys at creation time.

**Assignment:** When creating a Journey, specify `brief_pack_ids` to assign packs. `POST /api/journeys/` also requires `guide_id` and `playbook_id` (both specific version ids):

```json
POST /api/journeys/
{
  "objs": [{
    "name": "Onboarding Journey",
    "guide_id": "...",
    "playbook_id": "...",
    "brief_pack_ids": ["pack-uuid-1", "pack-uuid-2"]
  }]
}
```

**Retrieval:** Use the efficient endpoint to fetch all briefs for a journey:

```
GET /api/journeys/{journey_id}/briefs
```
<!-- ANCHOR_END: brief-pack -->

---

<!-- ANCHOR: debrief-ux -->
## Debrief UX Patterns

When designing debrief interviews:

1. **Never surface internal terms** like "Brief". Use user-friendly labels like "form" or "step".
2. **Structure questions** to minimize ambiguous free-text; prefer constrained inputs (selects, enums) where possible
3. **Respect BriefGroup atomicity:** Either all questions in the group are answered or none are accepted
4. **Consider iteration:** Users may return to debrief multiple times as the workflow progresses

**Anti-patterns:**
- Exposing "Brief" or "BriefPack" terminology to end users
- Allowing partial BriefGroup completion
- Designing one-shot flows that don't support iteration
<!-- ANCHOR_END: debrief-ux -->

---

<!-- ANCHOR: manifest -->
## Pack Manifest

The Pack Manifest is a YAML or JSON file at the root of a Brief Pack describing:

- Pack metadata (name, version, description)
- BriefGroups and their contained Briefs
- Required prompts and examples
- Dependencies and required binaries

See the [Brief Pack Manifest](../../journey-planner-guide/briefs/brief-pack-manifest.md) chapter for the full specification.
<!-- ANCHOR_END: manifest -->
