# Shared Content

> **Audience:** Guide Authors - Prompt Engineers

> **⚠ Status — deferred to V2+:** the `_shared/` content reference system is **not currently implemented** end-to-end. The ingest pipeline, parser, and database tables that materialized shared content were removed when the path layer was refactored. The `:::ref` directive is still parsed but currently has no materialization target. The chapter below describes the intended authoring model for when shared content returns; for now it is preserved as design context, not as a how-to.

Shared content allows multiple guides to reference common objects -- tasks, inference profiles, and Fluent messages -- without duplicating them. This chapter covers authoring shared content libraries, ingesting them, and referencing them from guides.

---

## Cross-Guide Content Sharing

Guides are self-contained by default. Each guide tarball carries its own stages, steps, tasks, inference profiles, and Fluent messages. But when multiple guides need the same validation task or the same inference profile, duplicating that content across every guide creates a maintenance burden.

Shared content solves this. You define reusable objects in a `_shared/` directory, ingest them once, and then reference them from any number of guides using `:::ref` blocks.

---

## Directory Structure

Shared content lives in `_shared/` directories organized by scope:

```
_shared/
  common/
    validate-output/
      shared.toml
      SUMMARY.md
      chapter-1.md
      locales/
        en.ftl
        pt-BR.ftl
    glossary/
      shared.toml
      SUMMARY.md
      terms.md
      locales/
        en.ftl
  internal/
    audit-prompts/
      shared.toml
      SUMMARY.md
      profiles.md
      locales/
        en.ftl
```

Each subdirectory under `common/` or `internal/` is a self-contained shared content unit with its own `shared.toml`, markdown chapters, and optional `locales/` directory.

### Scope Directories

| Directory | Scope | Visibility |
|-----------|-------|------------|
| `_shared/common/` | `common` | Available to all guides (public, partner, internal) |
| `_shared/internal/` | `internal` | Available only to internal guides |

The scope determines which guides can reference the shared content. Public guides cannot reference internal shared content. Internal guides can reference both scopes.

---

## The `shared.toml` File

Every shared content directory must contain a `shared.toml` file that provides metadata. This is analogous to `guide.toml` for regular guides.

```toml
name = "validate-output"
description = "Common output validation tasks and inference profiles"
scope = "common"
version = "1.3.0"
```

### Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Kebab-case identifier for the shared content unit |
| `scope` | Yes | Either `"common"` or `"internal"` |
| `description` | No | Human-readable description |
| `version` | No | Semantic version string (validated as semver when present) |

The `name` and `scope` fields combine to form the **ref key**: `{scope}/{name}`. For example, a shared content unit with `scope = "common"` and `name = "validate-output"` has the ref key `common/validate-output`. This ref key is how guides reference the shared content.

---

## Shared Content Structure

The internal structure of a shared content directory follows the same conventions as a guide directory. It contains markdown files with directives, a `SUMMARY.md` for chapter ordering, and optional `locales/` for Fluent messages.

However, not all guide features apply to shared content. The transform layer warns about and ignores objects that are not applicable:

| Object Type | Supported in Shared Content | Notes |
|-------------|---------------------------|-------|
| Tasks (`:::nushell`) | Yes | Reusable scripts |
| Inference profiles (`:::inference_profile`) | Yes | Reusable model configurations |
| Fluent messages (`locales/*.ftl`) | Yes | Localized text for shared objects |
| Asset files | Yes | Binary assets referenced by shared objects |
| Chapters and blocks | Yes | Stored for block extraction |
| Playbooks (`:::stage`/`:::step`) | No | Warning emitted, objects ignored |
| Brief packs (`:::brief_pack`) | No | Warning emitted, objects ignored |
| Refs (`:::ref`) | No | Warning emitted, objects ignored |

---

## Ingesting Shared Content

Shared content is ingested through a dedicated API endpoint, separate from guide ingest.

### Endpoint

```
POST /api/shared-content/ingest?scope=<common|internal>
Content-Type: multipart/form-data
```

The request body is a multipart form with the shared content directory packed as a tarball, following the same conventions as guide ingest.

### Query Parameters

| Parameter | Required | Description |
|-----------|----------|-------------|
| `scope` | Yes | Must be `common` or `internal`; must match `shared.toml` scope |

### SDK Usage

If using the Rust SDK:

```rust
let tarball_bytes = fs::read("validate-output.tar.gz")?;
client.shared_content().ingest(tarball_bytes, "common").await?;
```

### What Happens During Ingest

1. The tarball is extracted and validated (same security checks as guide ingest -- no symlinks, no path traversal)
2. `shared.toml` is parsed and validated
3. The directory is parsed as a guide structure (`parse_guide()`)
4. The parsed result is transformed into a `SharedContentPlan` via `plan_shared_content()`
5. The plan is materialized -- tasks, inference profiles, and Fluent messages are written to the database
6. The shared content is registered with its ref key (`{scope}/{name}`) for future lookups

---

## The SharedContentPlan

The transform layer produces a `SharedContentPlan` that contains the subset of objects relevant to shared content:

| Field | Type | Description |
|-------|------|-------------|
| `name` | `String` | From `shared.toml` |
| `description` | `Option<String>` | From `shared.toml` |
| `scope` | `String` | `"common"` or `"internal"` |
| `version` | `Option<String>` | Semantic version from `shared.toml` |
| `ref_key` | `String` | Constructed as `"{scope}/{name}"` |
| `chapters` | `Vec<ChapterDef>` | Parsed chapters for block extraction |
| `asset_files` | `Vec<AssetFileEntry>` | Discovered asset files |
| `derived_tasks` | `Vec<TaskDef>` | Tasks from `:::nushell` blocks |
| `derived_inference_profiles` | `Vec<InferenceProfileDef>` | Profiles from `:::inference_profile` blocks |
| `fluent_messages` | `HashMap<String, FluentMessageSet>` | Localized text keyed by message name |
| `warnings` | `Vec<String>` | Non-fatal issues (e.g., unsupported object types found) |

---

## Referencing Shared Content from Guides

Guides reference shared content using directive blocks with a `ref` attribute instead of a `name` attribute. The `ref` value is the shared content's ref key.

### The `:::ref` Block Syntax

Use a `:::ref` block when you want to pull in shared content by reference:

```markdown
:::ref { ref: "common/glossary" }
:::
```

The `ref` attribute is extracted by `extract_ref()` in the parser's attribute module. When a block has a `ref` attribute, the parser treats it as a shared reference rather than extracting a derived object.

### Directive Blocks with `ref` Attribute

Beyond `:::ref` blocks, any directive block can use a `ref` attribute to reference shared content instead of defining its own:

```markdown
:::inference_profile { ref: "common/validate-output" }
:::

:::nushell { ref: "internal/audit-prompts" }
:::
```

When the parser encounters a directive with a `ref` attribute, it creates a `SharedRefDef` entry instead of extracting the normal derived object (task, inference profile, etc.):

```
SharedRefDef {
    ref_key: "common/validate-output",
    directive_type: "inference_profile",
    source_block_index: BlockIndex { chapter_index: 1, block_index: 3 },
}
```

These shared references are collected in the `IngestPlan`'s `shared_refs` field and resolved during materialization.

---

## Scope Visibility Rules

Scope enforcement prevents accidental leakage of internal content into public-facing guides:

| Guide Classification | Can Reference `common` | Can Reference `internal` |
|---------------------|----------------------|------------------------|
| Public | Yes | No |
| Partner | Yes | No |
| Internal | Yes | Yes |

The guide's `classification` field (from `guide.toml`) determines which scopes it can reference. If a public guide attempts to reference `internal/audit-prompts`, the ingest pipeline will reject the reference with an error.

---

## Tracking in `rev.guide_shared_refs`

Each shared reference from a guide is recorded in the `rev.guide_shared_refs` table. This table tracks:

- Which guide version references which shared content
- The directive type of the reference
- The source block location within the guide

This tracking enables:

- **Impact analysis** -- When updating shared content, you can find all guides that reference it
- **Dependency graphs** -- Understanding which guides depend on which shared libraries
- **Scope validation** -- Verifying that all references comply with scope visibility rules

---

## Example: Complete Shared Content Workflow

### 1. Create the shared content

```
_shared/common/validate-output/
  shared.toml
  SUMMARY.md
  validation.md
  locales/
    en.ftl
```

**`shared.toml`:**

```toml
name = "validate-output"
description = "Common output validation task and inference profile"
scope = "common"
version = "1.0.0"
```

**`validation.md`:**

```markdown
# Output Validation

:::nushell { name: "validate-json-output" }
# Validates that task output is well-formed JSON
let input = $in
try {
    $input.payload | from json | ignore
    { payload: { valid: true, data: $input.payload }, signals: { validated: true } }
} catch {
    { payload: { valid: false, error: "Invalid JSON output" }, signals: { validated: false } }
}
:::

:::inference_profile { name: "output-quality-checker" }
:::
```

**`locales/en.ftl`:**

```ftl
output-quality-checker =
    Evaluate the quality and completeness of task output.
    .description = Output quality evaluation profile
    .system-prompt = You are a quality assurance checker. Given task output, evaluate whether it is complete, well-formed, and meets the expected schema. Return a JSON object with: quality_score (0-100), issues (array of strings), and pass (boolean).
    .temperature = 0.1
    .max-tokens = 512
```

### 2. Ingest the shared content

```bash
# Pack the directory
tar -czf validate-output.tar.gz -C _shared/common/ validate-output/

# Ingest via API
curl -X POST "http://localhost:8000/api/shared-content/ingest?scope=common" \
  -H "Authorization: Bearer $TOKEN" \
  -F "file=@validate-output.tar.gz"
```

### 3. Reference from a guide

In your guide's markdown:

```markdown
## Validation Step

This step uses the shared validation task to verify output quality.

:::nushell { ref: "common/validate-output" }
:::

:::inference_profile { ref: "common/validate-output" }
:::
```

The guide does not redefine these objects. It references the shared content by ref key, and the materialization handler resolves the references to the already-ingested shared objects.

---

## Best Practices

1. **Use `common` scope by default** -- Only use `internal` when the content genuinely should not be visible to public or partner guides. Broader scope means more reuse.

2. **Version your shared content** -- The `version` field in `shared.toml` enables idempotent ingest. Always increment it when changing the content so the ingest pipeline creates a new version.

3. **Keep shared content units small and focused** -- A shared content unit should define one logical group of related objects. `validate-output` (one task + one profile) is better than `common-utilities` (twenty tasks across unrelated domains).

4. **Name shared content descriptively** -- The `name` becomes part of the ref key that guide authors will type. `validate-output` is clearer than `utils-v2`.

5. **Document the contract** -- Shared tasks should include comments or prose blocks explaining their expected input and output shapes. Guide authors referencing them need to understand the interface.

6. **Check impact before updating** -- Before re-ingesting shared content with changes, query `rev.guide_shared_refs` to understand which guides depend on it. Breaking changes to shared content can affect multiple guides simultaneously.

7. **Provide all locales** -- If your shared content includes Fluent messages, provide translations for all locales that consuming guides might need. A guide expecting `pt-BR` translations will have gaps if the shared content only defines `en`.
