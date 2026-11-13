# Build a Resume -- Guide Authoring Walkthrough

> **Audience:** Guide Authors -- Prompt Engineers

This walkthrough builds a "Build a Resume" scenario using the guide authoring approach. You author a guide directory and ingest it as a tarball. The ingest pipeline parses the content, extracts derived objects (tasks, stages, steps, playbook), and materializes everything atomically.

---

## What We Are Building

A guide called "Build a Resume" that, when ingested, produces:

| Entity | Name | Source |
|--------|------|--------|
| Guide | Build a Resume | `guide.toml` + chapters |
| Task | `collect-personal-data` | `:::nushell` block |
| Task | `format-resume` | `:::nushell` block |
| Task | `render-pdf` | `:::nushell` block |
| Step | `collect-data-step` | `:::step` block |
| Step | `format-step` | `:::step` block |
| Step | `render-step` | `:::step` block |
| Stage | `build` | `:::stage` block |
| Stage | `review` | `:::stage` block |
| Playbook | `build-resume` | Derived from stage/step hierarchy |
| Inference Profile | `resume-reviewer` | `:::inference_profile` block |
| Brief Pack | `resume-input` | `:::brief_pack` block |
| Guide Card | -- | `[card]` section in `guide.toml` |

---

## Step 1: Create the Directory Structure

```
build-resume/
  guide.toml
  SUMMARY.md
  src/
    intro.md
    collecting-data.md
    formatting.md
    rendering.md
    review.md
  locales/
    en.ftl
  assets/
    images/
      resume-card.jpg
```

The `src/` directory contains chapter markdown files. The `locales/` directory contains Fluent translation files. The `assets/` directory holds images and other binary files that will be uploaded through the asset pipeline.

---

## Step 2: Write `guide.toml`

The guide metadata file defines the guide's identity and card configuration:

```toml
title = "Build a Resume"
description = "Builds a professional PDF resume from user input"
author = "Dapple Team"
classification = "workflow"
version = "1.0.0"

[card]
image = "assets/images/resume-card.jpg"
color = "#2D6A4F"
accent = "#95D5B2"
```

**Field reference:**

| Field | Required | Description |
|-------|----------|-------------|
| `title` | Yes | Guide title. Becomes the guide's display name and slug source. |
| `description` | No | Short description for search and card display. |
| `author` | No | Author name for attribution. |
| `classification` | No | Guide type: `"workflow"`, `"reference"`, `"tutorial"`, etc. |
| `version` | No | Semver string. Defaults to `0.0.1` if omitted. |
| `[card].image` | No | Relative path to the card image file within the guide directory. |
| `[card].color` | No | Primary card background color (CSS hex). |
| `[card].accent` | No | Secondary accent color (CSS hex). |

Note: The `[card]` section contains only locale-independent visual properties. Subtitle and category text are Fluent attributes on the `:::brief_pack` message (see Step 5).

---

## Step 3: Write `SUMMARY.md`

The `SUMMARY.md` file defines the chapter structure, mirroring mdBook conventions:

```markdown
# Summary

- [Introduction](./src/intro.md)
- [Collecting User Data](./src/collecting-data.md)
- [Formatting the Resume](./src/formatting.md)
- [Rendering to PDF](./src/rendering.md)
- [Human Review](./src/review.md)
```

Each line maps to a chapter in the ingested guide. Nesting (indentation) creates parent-child chapter relationships:

```markdown
- [Getting Started](./src/getting-started.md)
  - [Prerequisites](./src/prerequisites.md)
  - [Installation](./src/installation.md)
```

---

## Step 4: Write Chapter Files with Directives

Chapter files use fenced directive blocks (`:::directive { attrs }`) to define content that maps to platform entities.

### `src/intro.md` -- Prose and Brief Pack

```markdown
# Build a Resume

Welcome to the Build a Resume guide. This workflow collects your personal
information, formats it into a professional layout, and renders a PDF.

:::brief_pack { name: "resume-input" }
:::

:::inference_profile { name: "resume-reviewer" }
:::
```

The `:::brief_pack` block declares a brief pack named `resume-input`. Its content text, card subtitle, and card category are defined in the Fluent file (see Step 5), not inline.

The `:::inference_profile` block declares an inference profile named `resume-reviewer`. Its description, system prompt, and model parameters are Fluent attributes.

### `src/collecting-data.md` -- Stage, Step, and Nushell Task

```markdown
# Collecting User Data

:::stage { name: "build", tags: [universal] }

:::step { name: "collect-data-step", expose: ["personal_data"], tags: [agent] }

:::nushell { name: "collect-personal-data", version: "1.0.0", tags: [agent] }
def main [] {
    let out_dir = $env.OUT_DIR? | default "./out"
    mkdir $out_dir

    let input = $in | from json

    # Validate required fields
    let required = ["name", "email"]
    for field in $required {
        if ($input | get -i $field | is-empty) {
            error make { msg: $"Missing required field: ($field)" }
        }
    }

    $input | save --force $"($out_dir)/input.json"

    {
        payload: $input,
        signals: { validated: true, fields: ($input | columns | length) }
    }
}
:::

:::

:::
```

Key points about this chapter:

- The `:::stage` block opens a stage named `build`. Everything until the closing `:::` at the same nesting level belongs to this stage.
- The `:::step` block declares a step. Its tasks are the `:::nushell` blocks written **as children inside the step fence** -- this is the recommended "nested" form. Nested tasks are the step's private tasks, auto-scoped to the step name (`collect-data-step/collect-personal-data`), and run in document order. A step is an ordered set of tasks that runs as one fail-together unit; multiple tasks per step is the norm. (Data retrieval is just a task inside the step, never its own step.)
- The `:::nushell` block contains the actual script. It includes `name` and `version` attributes for slug-based deduplication -- if a task with the same slug and version already exists in the database, the existing task is reused.
- Nushell blocks retain their inline content (scripts are not localized). All other directive types defer their content to Fluent messages.
- The `tags` attribute controls audience filtering: `[agent]` means the block is relevant to LLM agents and technical consumers.

### `src/formatting.md` -- Second Step and Task

```markdown
# Formatting the Resume

:::step { name: "format-step", expose: ["resume_markdown"], tags: [agent] }

:::nushell { name: "format-resume", version: "1.0.0", tags: [agent] }
def main [] {
    let out_dir = $env.OUT_DIR? | default "./out"
    let in_artifact = $env.IN_ARTIFACT? | default "./out/artifact.json"
    mkdir $out_dir

    let prev = open $in_artifact
    let data = open $prev.path

    let md = $"# ($data.name)\n\n**Email:** ($data.email)\n\n## Work History\n\n"
        + ($data.work_history? | default [] | each {|job|
            $"### ($job.title) at ($job.company)\n*($job.start_date) - ($job.end_date? | default 'Present')*\n\n"
        } | str join "\n")

    $md | save --force $"($out_dir)/resume.md"

    {
        payload: { path: $"($out_dir)/resume.md" },
        signals: { formatted: true }
    }
}
:::

:::
```

### `src/rendering.md` -- Third Step and Task

```markdown
# Rendering to PDF

:::step { name: "render-step", expose: ["resume_pdf"], tags: [agent] }

:::nushell { name: "render-pdf", version: "1.0.0", tags: [agent] }
def main [] {
    let out_dir = $env.OUT_DIR? | default "./out"
    let in_artifact = $env.IN_ARTIFACT? | default "./out/artifact.json"
    mkdir $out_dir

    let prev = open $in_artifact
    let md_path = $prev.path
    let pdf_path = $"($out_dir)/resume.pdf"

    if (which pandoc | is-empty) {
        error make { msg: "pandoc not found" }
    }

    pandoc $md_path -o $pdf_path

    {
        payload: { path: $pdf_path, type: "application/pdf" },
        signals: { rendered: true }
    }
}
:::

:::

:::
```

Note the final closing `:::` at the end. This closes the `:::stage { name: "build" }` block that was opened in `collecting-data.md`. Because the parser processes chapters in SUMMARY.md order, the stage spans across multiple chapter files. The steps within these chapters become the stage's ordered step list.

### `src/review.md` -- Second Stage

```markdown
# Human Review

:::stage { name: "review", tags: [universal] }

This stage pauses execution for human review before the resume is finalized.
The reviewer can approve, request changes, or reject the generated PDF.

:::step { name: "review-step", tags: [universal] }
:::

:::
```

This creates a second stage with a single step. The review step nests no `:::nushell` tasks and references none -- it is a human checkpoint where the operator supplies the answer directly, rather than an automated task. (Steps that are meant to *run* tasks need at least one task: nested children, or a `tasks: [...]` reference; ingest rejects a task-less step that is not a pure operator-input checkpoint.)

---

## Step 5: Write the Fluent Locale File

Create `locales/en.ftl` with message keys matching the directive `name` attributes:

```ftl
# locales/en.ftl

# Stage descriptions
build =
    The build stage collects user data, formats it into a professional
    layout, and renders the final PDF.

review =
    Human review stage. A reviewer inspects the generated resume and
    approves or requests changes before finalization.

# Step descriptions
collect-data-step =
    Collect personal information and work history from the user.

format-step =
    Format the collected data into a Markdown resume layout.

render-step =
    Render the Markdown resume to a PDF document using pandoc.

review-step =
    Pause for human review and approval of the generated resume.

# Brief pack content
resume-input =
    The Resume Input pack collects your personal details and work history
    to generate a professional PDF resume.
    .card-subtitle = Ready to build your resume?
    .card-category = workflow

# Inference profile
resume-reviewer =
    Review generated resumes for completeness, formatting quality, and
    professional tone.
    .description = AI-assisted resume quality review
    .system-prompt = You are a professional resume reviewer. Evaluate the
        resume for completeness, formatting consistency, and professional
        tone. Provide specific, actionable feedback.
    .temperature = 0.3
    .max-tokens = 1024
```

**Key conventions:**

- Message keys use **kebab-case** and must match the `name` attribute in directive blocks.
- The message body (indented text after the key) becomes the localized content for that entity.
- **Fluent attributes** (lines starting with `.key = value`) carry structured metadata:
  - `.card-subtitle` and `.card-category` on brief pack messages provide card UI metadata.
  - `.description`, `.system-prompt`, `.temperature`, `.max-tokens` on inference profile messages configure the model.

To support additional locales, add more `.ftl` files (e.g., `locales/pt-BR.ftl`) with the same message keys and translated bodies.

---

## Step 6: Validate with a Dry Run

Before committing to a full ingest, run a dry-run validation to check for errors and review the planned entity counts.

### Using the Rust SDK

```rust
use dapple_sdk::{DappleClient, types::IngestParams};
use std::path::Path;

let client = DappleClient::with_api_token(
    "https://api.example.com",
    "dpl_your_token",
);

let response = client.guides().ingest_from_dir(
    &Path::new("./build-resume"),
    IngestParams { dry_run: true, source_path: None },
).await?;

println!("Status: {}", response.status);
println!("Details: {}", serde_json::to_string_pretty(&response.extra)?);
```

### Using curl

```bash
cd build-resume
tar czh -f ../build-resume.tar.gz .
cd ..

curl -X POST https://api.example.com/api/guides/ingest?dry_run=true \
  -H "Authorization: Bearer dpl_your_token" \
  -F "file=@build-resume.tar.gz"
```

### Expected Dry-Run Output

```json
{
  "valid": true,
  "errors": [],
  "warnings": [],
  "summary": {
    "title": "Build a Resume",
    "chapters": 5,
    "blocks": 12,
    "assets": 1,
    "derived_playbook_stages": 2,
    "derived_brief_packs": 1,
    "derived_tasks": 3,
    "derived_inference_profiles": 1,
    "deprecation_level": null,
    "deprecation_reason": null
  }
}
```

Review the summary to confirm the entity counts match your expectations. Check the `warnings` array for any issues with unnamed blocks, missing Fluent keys, or other non-fatal problems.

---

## Step 7: Full Ingest

Once validation passes, run the full ingest to materialize everything to the database.

### Using the Rust SDK

```rust
let response = client.guides().ingest_from_dir(
    &Path::new("./build-resume"),
    IngestParams {
        dry_run: false,
        source_path: Some("guides/build-resume".into()),
    },
).await?;

println!("Guide object_id: {:?}", response.extra.get("guide"));
println!("Derived objects: {:?}", response.extra.get("derived_objects"));
```

The `source_path` parameter records where this guide lives in your source repository. It is stored as metadata on the guide entity and used for traceability.

### Using curl

```bash
curl -X POST "https://api.example.com/api/guides/ingest?source_path=guides/build-resume" \
  -H "Authorization: Bearer dpl_your_token" \
  -F "file=@build-resume.tar.gz"
```

### Response

The response includes the created guide and all derived object IDs:

```json
{
  "status": "success",
  "guide": {
    "object_id": "019...",
    "path_object_id": "019...",
    "title": "Build a Resume",
    "slug": "build-resume",
    "version_major": 1,
    "version_minor": 0,
    "version_patch": 0,
    "chapters": [ ... ],
    "created_at": "2026-02-18T..."
  },
  "derived_objects": {
    "playbook_object_id": "019...",
    "task_object_ids": ["019...", "019...", "019..."],
    "inference_profile_object_ids": ["019..."],
    "brief_pack_object_ids": []
  }
}
```

---

## Step 8: Verify Derived Entities

After ingest, confirm that the derived entities were created correctly.

### Verify the Guide

```rust
let guide = client.guides().get_by_slug("build-resume").await?;
println!("Guide: {} (v{}.{}.{})",
    guide.title,
    guide.version_major,
    guide.version_minor,
    guide.version_patch,
);
```

### Verify Tasks

```rust
let tasks = client.tasks().list_by_slugs(
    vec![
        "collect-personal-data".into(),
        "format-resume".into(),
        "render-pdf".into(),
    ],
    None,
).await?;
println!("Tasks created: {}", tasks.len());
for task in &tasks {
    println!("  - {} ({})", task.name, task.object_id);
}
```

### Verify the Playbook

```rust
let playbooks = client.playbooks().list_by_slugs(
    vec!["build-resume".into()],
    None,
).await?;
if let Some(pb) = playbooks.first() {
    println!("Playbook: {} with {} stages",
        pb.name,
        pb.stages.len(),
    );
}
```

### Verify Guide Cards

```rust
let cards = client.guides().cards(None).await?;
for card in &cards {
    println!("Card: {} - color: {:?}", card.title, card.color);
}
```

---

## Step 9: Publish a New Version

When you update the guide content, ingest a new version under the existing path:

1. Update `guide.toml` with a new version:
   ```toml
   version = "1.1.0"
   ```

2. Make your content changes (edit chapters, add blocks, update locales).

3. Ingest as a PATCH:
   ```rust
   let tarball = std::fs::read("build-resume-v1.1.tar.gz")?;
   let response = client.guides().ingest_version(
       existing_path_object_id,
       tarball,
       IngestParams { dry_run: false, source_path: None },
   ).await?;
   ```

   Or via curl:
   ```bash
   curl -X PATCH "https://api.example.com/api/guides/<path_object_id>/ingest" \
     -H "Authorization: Bearer dpl_your_token" \
     -F "file=@build-resume-v1.1.tar.gz"
   ```

The path (and slug) remain stable. The new version is accessible via the same slug query, while older versions can be retrieved via `list_by_path_ids`.

---

## Guide Authoring vs Manual API Approach

| Aspect | Guide Authoring (this walkthrough) | Manual API (Nushell-first walkthrough) |
|--------|-----------------------------------|---------------------------------------|
| Entity creation | Automatic from directives | Explicit POST per entity |
| Localization | Built-in via Fluent `.ftl` files | Not applicable |
| Versioning | `guide.toml` version field | Manual PATCH calls |
| Deduplication | Automatic slug+version matching | Caller must check |
| Atomicity | Single transaction | Multiple API calls |
| Card creation | `[card]` section in `guide.toml` | Not covered |
| Asset upload | Automatic from `assets/` directory | Separate draft/chunk/finalize |
| Validation | `dry_run=true` pre-flight check | No equivalent |

The guide authoring approach is recommended for workflows where the content, tasks, and playbook structure are tightly coupled and authored together. The manual API approach is better suited for programmatic entity creation where each component is managed independently.

---

## Checklist

Before ingesting your guide, verify:

- [ ] `guide.toml` has a `title` field (required)
- [ ] `guide.toml` `version` is valid semver (if present)
- [ ] `SUMMARY.md` lists all chapter files with correct relative paths
- [ ] Every `:::nushell` block has a `name` attribute
- [ ] Every `:::stage` and `:::step` block has a `name` attribute
- [ ] Every step that runs tasks has at least one task -- nested `:::nushell` children (recommended), or a `tasks: ["a","b"]` / `task: "x"` reference to a `:::nushell` block name that exists in the guide
- [ ] `locales/en.ftl` contains a message key for every named non-nushell directive block
- [ ] Fluent message keys use kebab-case and match directive `name` attributes exactly
- [ ] Asset files referenced in `guide.toml` (card image) exist in the guide directory
- [ ] No symlinks in the directory (or use `tar czh` to dereference)
- [ ] Dry run passes with expected entity counts and no unexpected warnings
