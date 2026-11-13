# Simple Guide Directory

> **Audience:** Guide Authors - Prompt Engineers

This example shows the simplest possible guide that can be parsed and ingested by the platform. It is based on the actual test fixture at `crates/dapple-parser/tests/fixtures/simple-guide/`.

A minimal guide requires:

- A `guide.toml` metadata file
- A `SUMMARY.md` table of contents
- At least one markdown chapter file
- A `locales/` directory with at least one `.ftl` file (for Fluent-backed blocks)

---

## Directory Structure

```
simple-guide/
├── guide.toml
├── SUMMARY.md
├── preface.md
├── intro.md
├── glossary.md
└── locales/
    └── en.ftl
```

---

## File Contents

### guide.toml

The only required field is `title`. All other fields are optional.

```toml
title = "Simple Guide"
description = "A minimal guide example"
```

The parser also accepts `author`, `classification`, `version`, and a `[card]` section, but none are required for a minimal guide. See the [resume guide example](./resume-guide.md) for a fully populated `guide.toml`.

---

### SUMMARY.md

The `SUMMARY.md` file defines the chapter ordering and nesting. It follows mdBook conventions: top-level links become chapters, and indented list items become nested children.

```markdown
# Summary

[Preface](./preface.md)

- [Introduction](./intro.md)

# Reference

- [Glossary](./glossary.md)
```

Section headers (lines beginning with `#`) create visual grouping in the table of contents. The `[Preface](./preface.md)` link before the first list item creates a top-level unnested chapter. List items under a section header are grouped under that section.

---

### preface.md

A plain chapter with no directives. The parser treats its entire content as a single `prose` block.

```markdown
# Preface

This is a minimal guide demonstrating the basic structure.
```

---

### intro.md

This chapter demonstrates the core directive nesting pattern: a `:::stage` containing a `:::step` containing a `:::nushell` task. Plain markdown text between and around directives is captured as `prose` blocks.

```markdown
# Introduction

Welcome to the simple guide.

:::stage { name: "onboarding" }

:::step { name: "gather-info", tags: [agent, product] }

Please provide your business details.

:::nushell { name: "process-info", tags: [coder] }
def main [] { { payload: {}, signals: {} } }
:::

:::

:::

More prose after the stage.
```

Key observations about the directive syntax:

- **Opening fence:** `:::directive_type { key: "value", ... }` on its own line.
- **Closing fence:** `:::` on its own line (no directive name needed).
- **Nesting:** Stages contain steps, steps can contain nushell blocks. Each level requires its own closing `:::`.
- **Attributes:** The `{ ... }` block after the directive name uses a relaxed key-value syntax (not strict JSON). The `name` attribute is required for all directives except `prose`.
- **Tags:** The `tags` attribute accepts a bracketed list of tag identifiers. Tags enable audience-targeted filtering.

---

### locales/en.ftl

The Fluent file provides localized text for every named directive block. Each message key must match the `name` attribute of the corresponding directive in the markdown files.

```ftl
onboarding =
    The onboarding stage walks users through initial setup.
    .description = Initial user onboarding

gather-info =
    Gather basic information from the user.
    .description = Information gathering step

process-info =
    Process the gathered information.
```

Fluent syntax rules:

- **Message key** is the identifier before `=`. It must match the directive `name` exactly (kebab-case).
- **Message body** is the indented text following the `=`. This becomes the FluentMessage body for that locale.
- **Attributes** use the `.key = value` syntax on indented lines. Common attributes include `.description` for stages and steps, `.card-subtitle` and `.card-category` for brief packs, and `.system-prompt`, `.temperature`, `.max-tokens` for inference profiles.

---

### glossary.md

A plain reference chapter with no directives.

```markdown
# Glossary

- **Journey** -- A user's workflow instance
- **Playbook** -- A reusable workflow template
- **Brief** -- A data gathering unit
```

---

## What This Produces After Ingest

When this guide is ingested via `POST /api/guides/ingest`, the parser and transform layer produce the following entities:

### Guide Structure

| Entity | Count | Details |
|--------|-------|---------|
| Guide | 1 | Title: "Simple Guide", versioned with a path object |
| Chapters | 3 | Preface, Introduction, Glossary |
| Blocks | ~6 | Prose blocks from plain markdown, plus the stage/step/nushell directive blocks |

### Derived Objects

| Entity | Count | Details |
|--------|-------|---------|
| Playbook | 1 | Derived from the `:::stage` / `:::step` hierarchy |
| Stages | 1 | "onboarding" |
| Steps | 1 | "gather-info" (linked to the "onboarding" stage) |
| Tasks | 1 | "process-info" (nushell script: `def main [] { { payload: {}, signals: {} } }`) |
| Fluent Messages | 3 | One per named directive: "onboarding", "gather-info", "process-info" |

### Content Routing

The transform layer classifies blocks into two categories based on where their text content comes from:

| Block Type | Content Source | This Guide's Examples |
|------------|---------------|-----------------------|
| **Fluent-backed** | Text lives in `.ftl` files. The `IngestPlan` block has `content: None`. | `stage`, `step`, `prose` (when named) |
| **Inline** | Text lives in the markdown. The `IngestPlan` block has `content: Some(...)`. | `nushell` (code is never localized) |

This distinction is critical: the `name` attribute on a Fluent-backed block links it to a FluentMessage key. If the key is missing from the `.ftl` file, the block's body text in the markdown (if any) serves as a fallback. If both are empty, the block has no content.

### Key Rules

1. **Directive `name` must match `.ftl` message key.** The `name: "onboarding"` attribute on the `:::stage` directive corresponds to the `onboarding = ...` message in `en.ftl`. A mismatch means the block has no Fluent content for that locale.

2. **Nushell blocks retain inline content.** The script inside a `:::nushell` fence is the task's code. It is stored in the `IngestPlan` block's `content` field and later materialized as the task's script body. Nushell code is not localized.

3. **All nushell scripts must return `{ payload, signals }`.** This is the contract enforced by the execution engine. The `payload` field carries the task's output data; the `signals` field carries control-plane metadata.

4. **Tags propagate to blocks.** The `tags: [agent, product]` on the `:::step` directive are stored on the block entity, enabling consumers to filter content by audience.

5. **The guide structure is locale-independent.** Adding a `locales/pt-BR.ftl` file with the same message keys would make this guide available in Portuguese without changing any markdown files.
