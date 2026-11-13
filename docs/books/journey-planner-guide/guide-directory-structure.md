# Guide Directory Structure

> **Audience:** Guide Authors · Prompt Engineers · Agents

A guide is a self-contained directory that the platform parses, transforms, and materializes into database entities. This chapter covers the anatomy of that directory -- what files go where, what each file does, and how the on-disk structure maps to API objects after ingest.

---

## Overview

Every guide directory contains a small set of well-known files:

| File / Directory | Required | Purpose |
|------------------|----------|---------|
| `guide.toml` | Yes | Metadata: title, description, author, classification, version, card, deprecation |
| `SUMMARY.md` | Yes | Table of contents -- ordered list of chapters with nesting |
| `CONDUITS.md` | Yes | The conduit's guide frame -- conduit-facing context injected whole into every model prompt for this guide (see below) |
| `*.md` chapters | Yes (at least one) | Markdown content files referenced by SUMMARY.md |
| `locales/*.ftl` | No | Fluent translation files for localizable text |
| `assets/` | No | Non-markdown files (images, data files) bundled with the guide |
| `AGENTS.md` | No | Context for the *authoring agent* working on the guide -- never parsed, never ingested |

The guide structure is **locale-independent**. There is no locale field in `guide.toml`. All human-readable text that needs localization lives in Fluent `.ftl` files under `locales/`, not in the guide metadata or chapter markdown.

### CONDUITS.md — the conduit's guide frame

`CONDUITS.md` is a **required, root-level** file (no `src/` fallback; English,
not localized in V1). It is the guide's dedicated context document for the
**conduit** — the model executing this guide's prompts. At ingest its raw
markdown is stored on the guide *version* row, so every journey pinned to that
version injects exactly the frame that shipped with it, as a `# Guide Frame`
system message on every prompt.

Write it for the model, covering three things:

1. What this guide does, in the conduit's terms (not marketing copy).
2. **Its bindings** — every value slot, its type *in this guide's context*,
   and what the value means to downstream steps and the operator.
3. **How to read retrieved prose** — the interpretive key for the
   `:::prose { tags: [agent] }` excerpts RAG will surface (excerpt headers are
   deliberately neutral; this section is where source identity and register
   live).

`AGENTS.md` and `CONDUITS.md` are **siblings for different readers**:
`AGENTS.md` briefs the agent *building* the guide; `CONDUITS.md` briefs the
model *running* it. Neither is a subset of the other. When deriving a
CONDUITS.md you find a binding whose semantics you can't state crisply, that's
a documentation gap — fix the guide's `AGENTS.md` in the same pass.

An empty `CONDUITS.md` passes ingest with a warning; a missing one fails the
parse (`CONDUITS.md not found`) on ingest POST/PATCH, dry-run, and
`dapple guide validate` alike.

---

## Minimal Example

The simplest valid guide directory looks like this:

```
simple-guide/
├── guide.toml
├── SUMMARY.md
├── CONDUITS.md
├── preface.md
├── intro.md
└── glossary.md
```

With `guide.toml` containing just:

```toml
title = "Summary"
```

And `SUMMARY.md` defining the chapter order:

```markdown
# Summary

[Preface](./preface.md)

- [Introduction](./intro.md)

# Getting Started

- [Glossary](./glossary.md)
```

The chapter markdown files contain standard markdown and, optionally, `:::directive` fenced blocks.

---

## Full Example with Locales and Assets

A production guide typically includes locale files and assets:

```
essay-helper/
├── guide.toml
├── SUMMARY.md
├── CONDUITS.md
├── preface.md
├── intro.md
├── topic-selection.md
├── drafting.md
├── locales/
│   ├── en.ftl
│   └── pt-BR.ftl
└── assets/
    └── images/
        ├── card.jpg
        └── workflow-diagram.png
```

---

## How Files Map to Entities After Ingest

When a guide directory is uploaded via `POST /api/guides/ingest`, the parser reads the directory and the API materializes its contents into database entities. The mapping is:

| On-Disk File | Database Entity | Notes |
|--------------|----------------|-------|
| `guide.toml` | **Guide** (versioned path object) | Title, description, author, classification, version |
| Each `.md` file in SUMMARY.md | **Chapter** (owned dependent of Guide) | Title from SUMMARY.md link text, nesting from indentation |
| `:::directive` blocks in chapters | **Block** (owned dependent of Chapter) | directive_type, name, content, attributes, tags |
| `:::stage` / `:::step` hierarchy | **Playbook** with Stages and Steps | Derived from block nesting structure |
| `:::nushell` blocks | **Task** definitions | Script content retained inline |
| `:::brief_pack` blocks | **BriefPack** definitions | Content from Fluent messages |
| `:::inference_profile` blocks | **InferenceProfile** definitions | Configuration from Fluent attributes |
| `locales/*.ftl` | **FluentMessage** entities | One message set per key, across all locales |
| `guide.toml [card]` | **GuideCard** | Image path, color, accent color |
| Files in `assets/` | **Asset** entities (chunked storage) | Uploaded via draft/chunk/finalize pipeline |

The entire hierarchy -- guide, chapters, blocks, derived objects, assets, and Fluent messages -- is created atomically in a single database transaction.

---

## File-by-File Reference

### guide.toml

The metadata file for the guide. The only required field is `title`. All other fields are optional.

```toml
title = "Essay Helper"
description = "Guides users through the essay writing process"
author = "Dapple Team"
classification = "workflow"
version = "1.2.0"

[card]
image = "assets/images/card.jpg"
color = "#2D6A4F"
accent = "#95D5B2"
```

For the complete field reference, see [guide.toml Reference](./guide-toml-reference.md).

### SUMMARY.md

Defines the ordered list of chapters using mdBook-format markdown links. The parser extracts chapter titles, file paths, and nesting depth from this file.

**Format rules:**

- Each chapter is a markdown link: `[Title](./path.md)` or `- [Title](./path.md)`
- Nesting is controlled by indentation (2 spaces per level)
- Section headers (`# Section Name`) are visual separators only -- they are not parsed as chapters
- Blank lines and non-link lines are ignored
- Bare links (without bullet prefix) are always depth 0

**Example with nesting:**

```markdown
# Summary

[Preface](./preface.md)

- [Introduction](./intro.md)

# Authoring

- [Getting Started](./getting-started.md)
  - [Installation](./getting-started/install.md)
  - [Configuration](./getting-started/config.md)
- [Advanced Topics](./advanced.md)
  - [Custom Commands](./advanced/custom-commands.md)
    - [Security Policies](./advanced/custom-commands/security.md)
```

This produces chapters at depths 0, 0, 1, 1, 0, 1, 2, with parent relationships computed from the nesting structure. Depth is calculated as `leading_spaces / 2`.

**Legacy fallback:** If `SUMMARY.md` is not found at the guide root, the parser checks `src/SUMMARY.md` (mdBook legacy convention). The root location is preferred.

### Chapter Markdown Files

Each `.md` file listed in SUMMARY.md becomes a Chapter entity. Chapter files contain standard markdown mixed with `:::directive` fenced blocks.

Prose content between directives is automatically captured as `prose` blocks. Directive blocks are parsed into typed Block entities with their attributes and tags.

```markdown
# Topic Selection

This stage helps the user choose an essay topic.

:::stage { name: "Topic Selection" }

:::step { name: "Brainstorm", tags: [agent] }

Help the user brainstorm potential topics.

:::nushell { name: "generate-topics", tags: [coder] }
def main [] {
    let topics = $in | get context | get interests
    { payload: { topics: $topics }, signals: { count: ($topics | length) } }
}
:::

:::

:::
```

For the complete directive syntax reference, see [Directive Blocks](./directive-blocks.md).

### locales/ Directory

Contains Fluent Translation List (`.ftl`) files, one per locale. The filename stem becomes the locale tag (e.g., `en.ftl` maps to locale `"en"`, `pt-BR.ftl` maps to `"pt-BR"`).

Fluent message keys use **kebab-case** and correspond to the `name` attribute on directive blocks. Message bodies provide the localizable text content. Fluent attributes (`.key = value`) carry metadata like descriptions, card subtitles, and inference profile parameters.

```ftl
# locales/en.ftl
topic-selection =
    Guide the user from initial brainstorming through to a focused
    thesis statement.

brainstorm-topics =
    Generate a list of potential essay topics.
    .description = Extended brainstorm description

essay-helper-pack =
    The Essay Helper guide walks users through essay writing.
    .card-subtitle = Ready to write an essay?
    .card-category = researcher
```

During the transform phase, messages with the same key across multiple locale files are grouped into a `FluentMessageSet` -- one set per key, with bodies and attributes indexed by locale.

If the `locales/` directory does not exist, no Fluent messages are generated and all content falls back to inline markdown.

For the full localization model, see [Fluent Localization](./fluent-messages.md).

### assets/ Directory

Contains non-markdown, non-configuration files that should be uploaded as Asset entities alongside the guide. Common contents include images, data files, audio, and video.

The asset discovery process:

- Recursively scans the guide directory for files
- **Excludes:** `.md` files, `.toml` files, `.ftl` files, hidden files (starting with `.`), and the `locales/` directory
- **Includes:** Everything else (images, PDFs, CSVs, audio, video, etc.)
- Files are uploaded via the draft/chunk/finalize asset pipeline during materialization

Relative paths are preserved. For example, `assets/images/card.jpg` is stored with that relative path for later retrieval.

There is no required directory structure within `assets/` -- organize files however makes sense for your guide. The `[card] image` field in `guide.toml` references a relative path to one of these asset files.

---

## What Happens During Ingest

The ingest pipeline processes a guide directory in three phases:

1. **Parse** (`dapple-parser`): Reads `guide.toml`, parses `SUMMARY.md` to discover chapters, tokenizes each chapter file into blocks, discovers assets, and parses `locales/*.ftl` files. Produces a `ParsedGuide` AST.

2. **Transform** (`plan_guide()`): Walks the parsed AST and extracts derived objects -- playbook definitions from stage/step hierarchy, task definitions from nushell blocks, brief pack and inference profile definitions, cross-references, and Fluent message sets. Produces an `IngestPlan`. This phase is pure (no database access).

3. **Materialize** (API handler): Takes the `IngestPlan` and creates all database entities within a single transaction -- the guide itself, chapters, blocks, tags, derived objects, Fluent messages, assets, and the promotional card.

You can preview the parse and transform phases without materializing by using the `dry_run=true` query parameter on the ingest endpoint. This returns a validation summary with warnings, block counts, chapter counts, asset counts, and derived object counts.

---

## Common Mistakes

- **Missing guide.toml** -- The parser requires `guide.toml` (or the legacy `book.toml` fallback). A directory without either file will fail to parse.

- **Missing SUMMARY.md** -- The parser requires `SUMMARY.md` to know which chapter files to read. Without it, no chapters are discovered.

- **Chapter file referenced in SUMMARY.md but missing on disk** -- The parser logs a warning and produces an empty chapter. The guide still ingests, but the chapter has no blocks.

- **Putting localizable text in guide.toml** -- Fields like card subtitle and category belong in `.ftl` files as Fluent attributes, not in TOML. The `[card]` section only contains locale-independent visual properties (image path, color, accent).

- **Using `locale` in guide.toml** -- There is no `locale` field. The guide structure is locale-independent. If a legacy `locale` field is present in the TOML, it is silently ignored.

- **Unclosed `:::` fences** -- Every opening `:::directive` must have a matching closing `:::`. Unclosed fences produce parse errors. See [Directive Blocks](./directive-blocks.md) for syntax details.
