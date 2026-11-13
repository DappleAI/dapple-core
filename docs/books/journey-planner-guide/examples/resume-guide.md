# Resume Guide -- Full Example

> **Audience:** Guide Authors - Prompt Engineers

This example shows a complete guide directory that exercises every directive type the parser supports. It produces a playbook with multiple stages, tasks, a brief pack, an inference profile, a promotional card, and a full set of Fluent messages.

This replaces the earlier `resume-playbook.md` and `ordered-setup-step.md` examples, which predated the guide ingest system.

---

## Directory Structure

```
build-resume/
├── guide.toml
├── SUMMARY.md
├── intro.md
├── building.md
├── review.md
├── locales/
│   └── en.ftl
└── assets/
    └── images/
        └── resume-card.jpg
```

---

## File Contents

### guide.toml

A fully populated metadata file demonstrating all supported sections.

```toml
title = "Build a Resume"
description = "Answer a few questions and get a polished PDF resume."
author = "Product Team"
classification = "workflow"
version = "1.0.0"

[card]
image = "assets/images/resume-card.jpg"
color = "#2D6A4F"
accent = "#95D5B2"
```

Field reference:

| Field | Required | Purpose |
|-------|----------|---------|
| `title` | Yes | Guide title, also used as the derived playbook name |
| `description` | No | Short summary for listings and search results |
| `author` | No | Provenance metadata |
| `classification` | No | Freeform category string (e.g., "workflow", "reference") |
| `version` | No | Semantic version string, validated as semver (e.g., "1.0.0", "0.1.0-dev") |
| `[card]` | No | Promotional card for UI display |
| `card.image` | No | Relative path to an image file in the guide's `assets/` directory |
| `card.color` | No | Primary card background color (hex) |
| `card.accent` | No | Accent color (hex) |

Note that `card.subtitle` and `card.category` are not TOML fields. Under the Fluent-native model, these are Fluent attributes on the brief pack message (`.card-subtitle` and `.card-category` in the `.ftl` file). Legacy TOML files that still contain these fields will have them silently ignored.

---

### SUMMARY.md

Three chapters spanning the full resume workflow.

```markdown
# Summary

- [Introduction](./intro.md)

# Building

- [Building Your Resume](./building.md)

# Review

- [Review and Publish](./review.md)
```

---

### intro.md

The introductory chapter uses `:::prose` and `:::brief_pack` directives. This is the chapter users see first; it sets context and declares the brief pack that packages the workflow for the UI.

```markdown
# Introduction

:::prose { name: "resume-overview", tags: [product] }

Everyone needs a resume. This guide walks you through collecting your
work history, formatting it into clean Markdown, and rendering a
publication-ready PDF.

:::

## Get Started

:::brief_pack { name: "resume-builder-pack", tags: [product] }

This pack bundles the resume workflow with guided debriefs to collect
your information.

:::
```

The `:::prose` block above is Fluent-backed. Its `name` attribute (`resume-overview`) corresponds to a message key in `en.ftl`. The markdown content between the fences serves as a fallback if the Fluent message is missing, but the canonical text comes from the `.ftl` file.

The `:::brief_pack` block declares a brief pack definition. Its card subtitle and category metadata come from Fluent attributes (`.card-subtitle` and `.card-category` on the `resume-builder-pack` message in `en.ftl`), not from the markdown content.

---

### building.md

The core workflow chapter. It contains a `:::stage` with nested `:::step` blocks, each containing a `:::nushell` task. This is where the playbook's execution structure is defined.

```markdown
# Building Your Resume

:::stage { name: "data-collection" }

:::step { name: "collect-info", tags: [agent] }

Provide your name, email, phone number, and work history.

:::nushell { name: "collect-personal-data", tags: [coder] }
def main [] {
    let out_dir = $env.OUT_DIR? | default "./out"
    mkdir $out_dir

    let input = $in

    $input | save --force $"($out_dir)/input.json"

    {
        payload: { path: $"($out_dir)/input.json" }
        signals: { fields_collected: ($input | columns | length) }
    }
}
:::

:::

:::

:::stage { name: "formatting" }

:::step { name: "format-step", tags: [agent] }

Convert the collected data into a structured Markdown resume.

:::nushell { name: "format-resume", tags: [coder] }
def main [] {
    let out_dir = $env.OUT_DIR? | default "./out"
    let input = open $"($out_dir)/input.json"

    let header = $"# ($input.name)\n($input.email)"
    let experience = $input.work_history
        | each { |job| $"## ($job.role) at ($job.company)\n($job.years) years" }
        | str join "\n\n"

    let resume = $"($header)\n\n($experience)"
    $resume | save --force $"($out_dir)/resume.md"

    {
        payload: { path: $"($out_dir)/resume.md" }
        signals: { sections: ($input.work_history | length) }
    }
}
:::

:::

:::step { name: "render-step", tags: [agent] }

Render the Markdown resume into a PDF document.

:::nushell { name: "render-pdf", tags: [coder] }
def main [] {
    let out_dir = $env.OUT_DIR? | default "./out"
    let md_path = $"($out_dir)/resume.md"

    # In production this would call a real PDF renderer.
    # For this example we copy the markdown as a placeholder.
    cp $md_path $"($out_dir)/resume.pdf"

    {
        payload: { path: $"($out_dir)/resume.pdf", type: "application/pdf" }
        signals: { rendered: true }
    }
}
:::

:::

:::
```

Nesting structure summary for this file:

```
:::stage "data-collection"
  └── :::step "collect-info"
        └── :::nushell "collect-personal-data"

:::stage "formatting"
  ├── :::step "format-step"
  │     └── :::nushell "format-resume"
  └── :::step "render-step"
        └── :::nushell "render-pdf"
```

Each `:::nushell` block's script must return a `{ payload, signals }` record. The `payload` carries the task's output data (typically a path to an artifact). The `signals` carry control-plane metadata visible to the execution engine.

---

### review.md

The final chapter adds a review stage with an `:::inference_profile` block that configures an LLM for resume data extraction.

```markdown
# Review and Publish

:::stage { name: "review" }

:::step { name: "ai-review", tags: [agent] }

An AI reviewer extracts structured data from the resume and checks
for completeness before publishing.

:::inference_profile { name: "resume-extractor", tags: [agent] }

This profile configures the LLM used to extract structured fields
from the formatted resume text.

:::

:::

:::
```

The `:::inference_profile` block is Fluent-backed. Its operational parameters (system prompt, temperature, max tokens) are Fluent attributes on the `resume-extractor` message in `en.ftl`, not inline directive attributes. This allows the same profile to have locale-specific system prompts.

---

### locales/en.ftl

Every named directive block in the markdown files must have a corresponding message key here. The keys are listed in the same order as they appear in the guide for readability, but order does not matter to the parser.

```ftl
# -- intro.md --

resume-overview =
    Everyone needs a resume. This guide walks you through collecting
    your work history, formatting it into clean Markdown, and rendering
    a publication-ready PDF.

resume-builder-pack =
    This pack bundles the resume workflow with guided debriefs to
    collect your information.
    .card-subtitle = Build a polished resume in minutes
    .card-category = career

# -- building.md --

data-collection =
    The data collection stage gathers personal details and work history
    from the user through a structured debrief.
    .description = Collect personal data and work history

collect-info =
    Provide your name, email, phone number, and work history.
    .description = Information collection step

collect-personal-data =
    Read and validate user-provided personal data.

formatting =
    The formatting stage converts raw data into a polished document,
    first as Markdown and then as a rendered PDF.
    .description = Format and render the resume

format-step =
    Convert the collected data into a structured Markdown resume.
    .description = Markdown formatting step

format-resume =
    Transform structured input into a Markdown resume document.

render-step =
    Render the Markdown resume into a PDF document.
    .description = PDF rendering step

render-pdf =
    Convert Markdown to a publication-ready PDF file.

# -- review.md --

review =
    The review stage uses an AI model to extract structured data from
    the resume and validate completeness before publishing.
    .description = AI-assisted review and validation

ai-review =
    An AI reviewer checks the resume for completeness and extracts
    structured metadata.
    .description = Automated resume review

resume-extractor =
    Extract structured fields (name, email, skills, experience) from
    a formatted resume document.
    .description = Resume data extraction profile
    .system-prompt = You are a resume analysis assistant. Extract structured JSON from the provided resume text. Return fields: name, email, skills (array), experience (array of objects with role, company, years).
    .temperature = 0.2
    .max-tokens = 2048
```

Fluent attribute conventions by directive type:

| Directive | Common Attributes | Purpose |
|-----------|-------------------|---------|
| `stage` | `.description` | Short description for UI and agent queries |
| `step` | `.description` | Short description for UI and agent queries |
| `brief_pack` | `.card-subtitle`, `.card-category` | Card display metadata, localized per locale |
| `inference_profile` | `.description`, `.system-prompt`, `.temperature`, `.max-tokens` | LLM configuration parameters |
| `prose` | (none typical) | Body text is the primary content |
| `nushell` | (none typical) | Code is not localized; the Fluent body serves as documentation |

---

### assets/images/resume-card.jpg

A JPEG image file referenced by `guide.toml`'s `[card]` section. During ingest, asset files are discovered by scanning the guide directory for non-markdown, non-TOML, non-FTL files. They are uploaded through the draft/chunk/finalize pipeline and associated with the guide entity.

The image path in `guide.toml` (`assets/images/resume-card.jpg`) is relative to the guide root directory.

---

## Entities Produced After Ingest

When this guide is ingested via `POST /api/guides/ingest` (as a tarball), the following entities are created in a single atomic transaction.

### Guide Structure

| Entity | Count | Details |
|--------|-------|---------|
| Guide (path + version) | 1 | Title: "Build a Resume", version 1.0.0, classification: "workflow" |
| Chapters | 3 | Introduction, Building Your Resume, Review and Publish |
| Blocks | ~12 | Prose, brief_pack, stage, step, nushell, and inference_profile blocks |

### Derived Playbook

The playbook is derived from the `:::stage` / `:::step` hierarchy across all chapters. The playbook name defaults to the guide title.

```
Playbook: "Build a Resume" (version 1.0.0)
├── Stage: "data-collection"
│   └── Step: "collect-info"
├── Stage: "formatting"
│   ├── Step: "format-step"
│   └── Step: "render-step"
└── Stage: "review"
    └── Step: "ai-review"
```

### Derived Tasks

| Task Name | Source Chapter | Script Summary |
|-----------|---------------|----------------|
| `collect-personal-data` | building.md | Reads input, saves to `input.json`, returns path in payload |
| `format-resume` | building.md | Reads `input.json`, produces `resume.md`, returns path in payload |
| `render-pdf` | building.md | Reads `resume.md`, produces `resume.pdf`, returns path in payload |

Tasks are linked to steps by proximity in the block hierarchy. Each `:::nushell` block inside a `:::step` becomes the task for that step.

### Derived Brief Pack

| Name | Fluent Attributes |
|------|-------------------|
| `resume-builder-pack` | `.card-subtitle = "Build a polished resume in minutes"`, `.card-category = "career"` |

The brief pack is declared in `intro.md`. Its display metadata comes from Fluent attributes, making it localizable without changing the markdown source.

### Derived Inference Profile

| Name | Fluent Attributes |
|------|-------------------|
| `resume-extractor` | `.system-prompt = "You are a resume analysis assistant..."`, `.temperature = "0.2"`, `.max-tokens = "2048"` |

The inference profile configures how the conduit layer routes prompt execution. The system prompt, temperature, and max tokens are locale-specific Fluent attributes, allowing different locales to have different prompting strategies.

### Promotional Card

| Field | Value | Source |
|-------|-------|--------|
| `image` | `assets/images/resume-card.jpg` | `guide.toml [card]` |
| `color` | `#2D6A4F` | `guide.toml [card]` |
| `accent` | `#95D5B2` | `guide.toml [card]` |
| `subtitle` | "Build a polished resume in minutes" | `en.ftl` attribute on `resume-builder-pack` |
| `category` | "career" | `en.ftl` attribute on `resume-builder-pack` |

The card combines locale-independent visual properties from `guide.toml` with locale-dependent text from Fluent messages.

### Fluent Messages

| Message Key | Body Summary | Attributes |
|-------------|-------------|------------|
| `resume-overview` | Guide overview prose | (none) |
| `resume-builder-pack` | Pack description | `.card-subtitle`, `.card-category` |
| `data-collection` | Stage description | `.description` |
| `collect-info` | Step description | `.description` |
| `collect-personal-data` | Task documentation | (none) |
| `formatting` | Stage description | `.description` |
| `format-step` | Step description | `.description` |
| `format-resume` | Task documentation | (none) |
| `render-step` | Step description | `.description` |
| `render-pdf` | Task documentation | (none) |
| `review` | Stage description | `.description` |
| `ai-review` | Step description | `.description` |
| `resume-extractor` | Profile description | `.description`, `.system-prompt`, `.temperature`, `.max-tokens` |

Total: 13 Fluent messages materialized as FluentMessage path + version entities.

### Asset Files

| File | MIME Type | Purpose |
|------|-----------|---------|
| `assets/images/resume-card.jpg` | `image/jpeg` | Card image, uploaded through draft/chunk/finalize pipeline |

---

## Adding a New Locale

To make this guide available in another language, add a new `.ftl` file under `locales/`. For example, `locales/es.ftl`:

```ftl
resume-overview =
    Todo el mundo necesita un curriculum. Esta guia le ayuda a recopilar
    su historial laboral, formatearlo en Markdown limpio y generar un
    PDF listo para publicar.

resume-builder-pack =
    Este paquete incluye el flujo de trabajo de curriculum con
    entrevistas guiadas para recopilar su informacion.
    .card-subtitle = Cree un curriculum profesional en minutos
    .card-category = carrera
```

The guide structure (markdown files, `SUMMARY.md`, `guide.toml`) does not change. Only the `.ftl` files are added or updated. Each FluentMessageSet will then contain bodies for both `en` and `es` locales, and the platform will select the appropriate locale at render time.

---

## Dry Run Validation

Before committing to a full ingest, use the `dry_run=true` query parameter to validate the guide without materializing any database entities:

```
POST /api/guides/ingest?dry_run=true
Content-Type: multipart/form-data
```

The response includes counts of chapters, blocks, assets, and derived objects, plus any warnings generated during parsing and transformation. This is useful for catching missing Fluent keys, unnamed directive blocks, or stages without steps before they reach the database.
