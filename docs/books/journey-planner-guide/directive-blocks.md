# Directive Blocks

> **Audience:** Guide Authors · Prompt Engineers · Agents

> **Terminology:** This chapter covers **block directive types** -- the `:::directive` fence syntax used to classify content within guide chapters. Block directives describe *what kind of information* a block contains (a stage definition, a nushell script, an inference profile). This is distinct from **flow-control directives**, which are JSONLogic rules that control stage and step execution at runtime. These are separate systems that share the word "directive." For flow-control directives, see [Directives and JSONLogic](./directives.md).

Directive blocks are the fundamental authoring primitive for guide content. They let you embed structured, typed content within standard markdown chapters. The parser recognizes directive fences, extracts their metadata, and transforms them into Block entities and derived objects during ingest.

---

## Syntax

A directive block is a fenced region within a markdown chapter file. The opening fence specifies the directive type and optional attributes. The closing fence is a bare `:::`.

```
:::type { key: "value", tags: [tag1, tag2] }

Content goes here.

:::
```

**Parts of a directive block:**

| Part | Description |
|------|-------------|
| `:::` | The fence delimiter (three colons). Opens or closes a block. |
| `type` | The directive type keyword. Must be one of the known types (see below). |
| `{ ... }` | Optional attribute object. Contains key-value metadata for the block. |
| Content | The text between the opening and closing fences. May be empty. |

---

## Attribute Syntax

The attribute object after the directive type accepts two syntaxes. Both produce the same parsed result.

### YAML-like syntax (preferred for readability)

```
{ name: "Gather Info", tags: [agent, product] }
```

- Keys do not need quotes: `name:` is equivalent to `"name":`
- Array values without quotes are auto-quoted: `[agent, product]` becomes `["agent", "product"]`
- String values without quotes are auto-quoted: `{ name: my-task }` becomes `{ "name": "my-task" }`
- Booleans and numbers remain unquoted: `{ enabled: true, count: 42 }` becomes `{ "enabled": true, "count": 42 }`
- The literals `true`, `false`, `null`, and any valid number are recognized as JSON primitives

### Standard JSON syntax

```
{ "name": "Gather Info", "tags": ["agent", "product"] }
```

Standard JSON is always accepted. If the attribute string parses as valid JSON directly, it is used as-is. The YAML-like normalization only runs when direct JSON parsing fails.

### Empty attributes

If a directive has no attributes, the `{ }` can be omitted entirely:

```
:::prose

Some narrative text.

:::
```

This produces an empty attribute object `{}`.

---

## Known Directive Types

The parser recognizes these directive types (defined in `KNOWN_DIRECTIVES`):

| Type | Aliases | Content Source | Produces |
|------|---------|----------------|----------|
| `prose` | -- | Fluent-backed | Block entity |
| `stage` | -- | Fluent-backed | Block + Stage in derived Playbook |
| `step` | -- | Fluent-backed | Block + Step in derived Playbook |
| `nushell` | -- | Inline | Block + TaskDef |
| `brief_pack` | `brief-pack` | Fluent-backed | Block + BriefPackDef |
| `inference_profile` | `inference-profile` | Fluent-backed | Block + InferenceProfileDef |
| `ref` | -- | Inline | Block + RefDef |
| `block` | -- | Fluent-backed | Block entity |

Hyphenated aliases (`brief-pack`, `inference-profile`) are normalized to underscores during parsing. Both forms are equivalent: `:::brief-pack` and `:::brief_pack` produce the same `directive_type: "brief_pack"`.

> **Brief-system directives.** The table above lists the top-level block types. The **brief system** adds a second family of directives — `:::value_slot`, `:::brief-input`, `:::brief-output`, and the container/display units `:::brief`, `:::brief-group`, `:::brief-notice`, `:::brief-markdown` — that the transform layer recognizes and materializes into the debrief form, value slots, and bindings. They are documented in [Brief-system directives](#brief-system-directives) below, and in depth (with the generate → pick → refine and **keep-N / multi-select** flows) in [Value-Slot Overlays](./value-slot-overlays.md). Hyphen and underscore spellings are equivalent here too (`:::value_slot` ≡ `:::value-slot`, `:::brief-input` ≡ `:::brief_input`).

---

## Fluent-Backed vs Inline Content

Directive blocks fall into two categories based on where their authoritative content comes from.

### Fluent-backed directives

**Types:** `prose`, `stage`, `step`, `brief_pack`, `inference_profile`, `block`

The authoritative content for these directives lives in Fluent `.ftl` files under `locales/`. The message key in the `.ftl` file matches the `name` attribute on the directive. During the transform phase, the block's inline markdown content is treated as fallback -- if the `.ftl` file provides a body for the message key, the Fluent text is the source of truth.

In the `IngestPlan`, Fluent-backed blocks have `content: None` when the inline markdown is empty. If the markdown contains non-empty text, it is retained as `content: Some(...)` for fallback purposes.

### Inline directives

**Types:** `nushell`, `ref`

These directives always retain their inline content. Nushell blocks contain code that is not localized. Ref blocks contain structural cross-references. Their content is stored directly in the Block entity.

---

## Directive Types in Detail

### prose

Narrative text, explanations, and descriptions. Prose blocks are the default -- any markdown content between directive fences (or outside any fence) is captured as a prose block.

```
:::prose { name: "introduction", tags: [agent, product] }

Welcome to the Essay Helper guide. This workflow walks you through
the complete essay writing process, from topic selection through
final editing.

:::
```

Implicit prose blocks are also created for markdown content that appears outside of any explicit directive fence. These implicit blocks have no `name` and an empty attribute object.

### stage

Defines a stage within a playbook. A **stage is a static form / pane** the user lands on and advances *between*. It contains one or more steps (the dynamic sub-interactions on that pane). Multi-step stages are the norm.

```
:::stage { name: "Topic Selection" }

:::step { name: "Brainstorm" }
Help the user brainstorm potential topics.
:::

:::step { name: "Narrow Down" }
Guide the user to select their best topic.
:::

:::
```

**Attributes:**

| Key | Required | Description |
|-----|----------|-------------|
| `name` | Yes | Stage name. Must match a Fluent message key in `.ftl` files. |
| `kind` | No | What kind of pane this stage is, from a closed vocabulary: `intake`, `fetch`, `analyze`, `decide`, `review`, `compare`, `assemble`, `deliver`. Semantic, not pictorial — clients map it to their own icon/presentation; journey status endpoints (`/steps`, `/stages`) carry it per stage. An unrecognized value produces an ingest warning and is stored as absent (clients render their neutral fallback). Folds into the stage content hash, so adding or changing a kind re-versions the stage on re-ingest; a stage with no `kind` keeps its existing hash. |
| `icon` | No | Up to **five** icon names, in your order, distinguishing this stage from its siblings *within* a kind (a fifteen-stage guide can honestly be nine `analyze` panes; `kind` alone draws nine identical discs). **Open vocabulary** — nothing is validated, and an unrecognized name is not an error: a drawing client falls back to the `kind` glyph, and a client that draws nothing ignores it. Accepts an array or a bare string (`icon: "radar"`). A client reads the first five and ignores the rest — authoring more warns at ingest but stores all of them. Order matters (it decides which five are read), so it is kept verbatim: no sorting, no dedup. Folds into the stage content hash, so adding, changing, **or reordering** icons re-versions the stage; a stage with no `icon` keeps its existing hash. See [Stage icon names](#stage-icon-names). |
| `tags` | No | Audience tags for filtering. |
| `version` | No | Semver string for idempotent ingest deduplication. |
| `expose` / `mask` | No | Value-slot names the stage needs / hides. Drives per-stage completion and the operator form — see [Value-Slot Overlays](./value-slot-overlays.md). Without them the stage has no gating. Use stage-level `expose` only for a slot the *whole* stage shares; slots a specific step needs or produces belong on the `:::step`. |

Stage description text comes from the Fluent message body matching the `name` key.

#### Stage icon names

```
:::stage { name: "Pull the Record", kind: intake, icon: [radar, scan-search, id-card] }
```

Hyphens are safe unquoted — the attribute parser only breaks bare words on
`{ } [ ] : ,` and whitespace, so `icon: [file-text, chart-line]` round-trips as
written. Quote a name only if you need a space or a comma in it (you never
will).

The names come from the drawing client's registry, **not** from this server:
dapple-ui curates a set of ~150 [lucide](https://lucide.dev) names grouped by
the `kind` they read as, and that grouping is advisory — nothing stops `radar`
on a `deliver` stage. Give several names when a stage has no single obvious
glyph; the client picks one deterministically per stage, so the disc is stable
for an operator sitting on it and the variety lands across stages.

Because the vocabulary is open, **a name this platform has never heard of is
normal, not broken.** It is stored exactly as you wrote it and simply falls back
to the `kind` glyph until the client learns it. That is also why an unknown
`icon` — unlike an unknown `kind` — produces no ingest warning.

### step

Defines a step within a stage — a **dynamic sub-interaction** (one round-trip of work + any operator input) before the user navigates on. Steps are the AJAXy units within a stage's pane; the canonical multi-step shape is generate → pick → refine.

```
:::step { name: "Gather Info", tags: [agent, product] }

Please provide your business details.

:::nushell { name: "process-info", tags: [coder] }
def main [] {
    let details = $in | get context
    { payload: $details, signals: { collected: true } }
}
:::

:::
```

**Attributes:**

| Key | Required | Description |
|-----|----------|-------------|
| `name` | Yes | Step name. Must match a Fluent message key in `.ftl` files. |
| `tags` | No | Audience tags for filtering. |
| `version` | No | Semver string for idempotent ingest deduplication. |
| `expose` / `mask` | No | Value-slot names this step needs **or produces**, overriding the stage's set for this step's tasks — see [Value-Slot Overlays](./value-slot-overlays.md). In a multi-step stage, a computed `:::brief-output` MUST be exposed here (on its producing step), not at the stage level — that is how the runtime knows which step owns it. |

Step description text comes from the Fluent message body matching the `name` key.

### nushell

Contains a Nushell script. The content is retained inline (not Fluent-backed) because code is not localized.

```
:::nushell { name: "format-resume", tags: [coder] }
def main [] {
    let input = $in | get data
    let formatted = $input | select name email skills | to md
    { payload: { resume: $formatted }, signals: { sections: 3 } }
}
:::
```

**Attributes:**

| Key | Required | Description |
|-----|----------|-------------|
| `name` | Yes | Task name. Becomes the `TaskDef` name in the ingest plan. |
| `tags` | No | Audience tags for filtering. |
| `version` | No | Semver string for idempotent ingest deduplication. |

All nushell scripts must return a `{ payload, signals }` record. See [Nushell Task Template](./nushell-task-template.md) for the full contract.

### brief_pack

Defines a brief pack -- a collection of briefs presented to users during a journey. The content text and card metadata (subtitle, category) come from Fluent messages and attributes.

```
:::brief_pack { name: "resume-builder-pack", tags: [product] }
:::
```

With corresponding Fluent message in `locales/en.ftl`:

```ftl
resume-builder-pack =
    Build a professional resume with guided input collection.
    .card-subtitle = Ready to build your resume?
    .card-category = career
```

**Attributes:**

| Key | Required | Description |
|-----|----------|-------------|
| `name` | Yes | Brief pack name. Must match a Fluent message key. |
| `tags` | No | Audience tags for filtering. |

### inference_profile

Defines an inference profile -- a configuration that maps prompts to model releases with sampling parameters. Description, system prompt, temperature, and max tokens come from Fluent message body and attributes.

```
:::inference_profile { name: "resume-extractor", tags: [agent] }
:::
```

With corresponding Fluent message in `locales/en.ftl`:

```ftl
resume-extractor =
    Extract structured resume data from free-form text input.
    .description = Resume data extraction profile
    .system-prompt = You are a resume parsing assistant. Extract structured data.
    .temperature = 0.3
    .max-tokens = 2048
```

**Attributes:**

| Key | Required | Description |
|-----|----------|-------------|
| `name` | Yes | Profile name. Must match a Fluent message key. |
| `tags` | No | Audience tags for filtering. |

### ref

A cross-reference to another document, guide, or section. Content is retained inline (not Fluent-backed) because references are structural, not localized.

```
:::ref { slug: "api-patterns", type: "guide" }
See the API Patterns guide for batch request semantics.
:::
```

**Attributes:**

| Key | Required | Description |
|-----|----------|-------------|
| `slug` | Yes | The slug of the referenced object. |
| `type` | No | The type of object being referenced (default: `"guide"`). |
| `tags` | No | Audience tags for filtering. |

### block

A generic Fluent-backed block for content that does not fit a more specific directive type.

```
:::block { name: "safety-notice", tags: [agent, product] }
:::
```

The block's content comes from the Fluent message body matching the `name` key.

---

## Brief-system directives

The brief system turns operator interaction into typed, bindable data. These directives are recognized by the transform layer (not in the top-level `KNOWN_DIRECTIVES` list) and materialize into value slots, the debrief form, and dispatch bindings. The narrative treatment — including the generate → pick → refine and **keep-N / multi-select** patterns — is in [Value-Slot Overlays](./value-slot-overlays.md); this is the attribute reference.

### value_slot

Declares a typed data slot that brief inputs collect, brief outputs produce, and overlays (`expose`/`mask`) gate on. The slot's `name` is its identity across versions; brief inputs/outputs reference it by that name.

```
:::value_slot { name: "kept_suppliers", kind: "text", cardinality: "multi", min: 0, max: 8 }
:::
```

| Key | Required | Description |
|-----|----------|-------------|
| `name` | Yes | Slot identity. Brief inputs/outputs and `expose`/`mask` reference this name. |
| `kind` | No (default `text`) | Value type: `text`, `int`, `number`, `bool`, `json`, `asset`. |
| `cardinality` | No (default `single`) | `single` = exactly one answer. `multi` (alias `keep-n`) = a **keep-N subset**: the slot holds N items of `kind` and the consuming task receives an **array**. |
| `min` / `max` | No (`multi` only) | Selection bounds for a `multi` slot. `min: 0` allows a deliberate *keep-none*; omit `max` for unbounded. Ignored on `single`. (`min_selections` / `max_selections` long-form spellings also accepted.) |
| `constraints` | No | JSON validation rules carried on the slot. |

### brief-input

An operator-fillable control inside a brief. Its `slot` names the value slot it writes; the control flavor decides the widget.

```
# single pick
:::brief-input { name: "pick_thesis", control: "select", slot: "chosen_thesis" }
:::

# keep-N pick over runtime-generated tiles
:::brief-input { name: "keep_suppliers", control: "checkbox",
                 slot: "kept_suppliers", candidate_source: "supplier_candidates" }
:::
```

| Key | Required | Description |
|-----|----------|-------------|
| `name` | Yes | Brief-input identity (and Fluent key for its label/help, see [Fluent Localization](./fluent-localization.md)). |
| `control` | No (default `text`) | Widget: `text`, `textarea`, `select`, `radio`, `checkbox`, `number`, `date`, `file`. **`checkbox` is the multi control** — pair it with a `cardinality: "multi"` slot. |
| `slot` | No | Name of the value slot this input writes (the pin target). A pick must have one. |
| `candidate_source` | No | Name of a **computed `kind: "json"` value slot** whose bound value supplies this control's choices at runtime (the dynamic generate → curate → refine candidate set). When set, tiles come from that slot's binding instead of authored choices. (`candidate-source` spelling also accepted.) |

> A pick is **always** a `:::brief-input`, never a bare exposed slot — a bare exposed pick slot has no bind target and the operator has nothing to render (ingest warns). For keep-N the control is `checkbox` + `candidate_source`.

### brief-output

A display/produced unit inside a brief. With `source: "computed"` it is **produced by a task** (the runtime extracts its value from the task payload and binds the slot); other sources echo operator input or uploaded files.

A computed output must be **named**, **listed in its `:::brief`'s `units`**, and
have its **slot exposed on the producing step** — all three, each silent if
missing. Author it as a top-level block referenced from the brief's `units`, not
an unnamed block nested under the step.

```
# computed candidate set: the generate task returns { payload: { choices: [...] } }
:::brief-output { name: "supplier-candidates-output", source: "computed",
                  slot: "supplier_candidates", source_path: "/choices" }
:::
:::brief { name: "extract-suppliers-brief", units: ["supplier-candidates-output"] }
:::
:::step { name: "Extract suppliers", task: "extract-suppliers", expose: ["supplier_candidates"] }
:::
```

| Key | Required | Description |
|-----|----------|-------------|
| `name` | **Yes** | Output identity and the unit key a `:::brief`'s `units` references. An unnamed brief-output is **dropped at parse**, so it never materializes. |
| `slot` | Yes (for computed/output) | Value slot this output reads from / produces. |
| `source` | No (default `input`) | `input` (echo an operator answer), `files` (uploaded assets), or `computed` (produced by a task). |
| `source_path` | No (computed) | RFC-6901 JSON pointer into the task payload (e.g. `/choices`). Empty / `.` = the whole payload. A `multi` computed slot whose `source_path` yields an **array** produces one item per element. |
| `display` | No (default `text`) | How a client renders the bound value. Closed vocabulary: `image`, `audio`, `video`, `text`, `markdown`, `list`, `checklist`, `table`, `fields`, `file`, `document`, `hidden`. `hidden` marks a data-flow slot that must be produced and bound but never shown. An unknown key warns at ingest and falls back to `text`. |
| `reveal` | No (default `false`) | Collapse the rendered block behind a "Reveal" spoiler toggle. |
| `title` | No | Heading label for the block. |
| `group` | No | Marks this output as one **format of a shared deliverable** — see below. |
| `format` | No | Short label naming this output's format within its `group` (`"html"`, `"pdf"`, …). Open vocabulary. |

A computed output binds only when listed in its `:::brief`'s `units` **and** its
slot is `expose`d on the producing step — see [Value-Slot Overlays → Computed outputs](./value-slot-overlays.md#computed-outputs-are-produced-by-a-step-not-bound-by-an-operator).

#### One deliverable, several formats

When a guide renders one artifact in several formats it exposes one computed
output per format. Give them a shared `group` and a `format` each, and a client
renders **one block with a format switcher** instead of unrelated-looking
sections:

```
:::brief-output { name: "report-src-output", source: "computed", slot: "tk_report",
                  source_path: "/report", display: "hidden", group: "tk_report" }
:::
:::brief-output { name: "report-html-output", source: "computed", slot: "tk_report_html",
                  source_path: "/report_html", display: "document",
                  title: "Read the report", group: "tk_report", format: "html" }
:::
:::brief-output { name: "report-pdf-output", source: "computed", slot: "tk_report_pdf",
                  source_path: "/report_pdf", display: "file",
                  title: "Download the report", group: "tk_report", format: "pdf" }
:::
```

- `group` is **opaque** — any stable string; the platform never parses it. Two
  outputs with the same token, rendered on the same pane, are the same
  deliverable. Omit it and the output is never grouped (the default).
- `format` is a **display label, not a mime type**, and deliberately not a closed
  set: coin `epub` and it reaches the client intact.
- A `hidden` sibling (the source the other formats are rendered from) may carry
  the `group`. Clients drop hidden slots *before* grouping, so it never becomes a
  third switcher entry.
- **Which format leads is authored order** — the first non-hidden member of the
  group. There is no `primary` attribute; move the block to change the lead.

#### Authored order is the pane order

`GET /api/journeys/{id}/steps` returns a step's exposed slots in the order their
brief units were **authored** — inputs and outputs interleaved exactly as
written, not grouped by kind and not sorted by slot name. Reordering
`:::brief-output` / `:::brief-input` blocks (or the brief's `units` list) is
therefore how you lay out a pane, and it re-versions the brief like any other
content change.

### Container & display units

`:::brief` groups units; `:::brief-group` groups briefs; `:::brief-notice` and `:::brief-markdown` are display-only (a notice level or inline markdown). These carry `name` (Fluent-backed title/body) and structural references; see [Value-Slot Overlays](./value-slot-overlays.md) and the brief-pack docs for how they assemble into the debrief.

---

## Nesting Rules

Directives can be nested to express containment relationships:

- **Stages contain steps.** A `:::stage` block (a form / pane) contains one or more `:::step` blocks (its sub-interactions) as children. Multiple steps per stage is the norm — generate → pick → refine.
- **A step contains one or more tasks.** A step is an ordered set of tasks executed as one fail-together unit; multiple tasks per step is the norm (data retrieval/fetch/lookup is a task *inside* a step, never its own step). There are two ways to give a step its tasks, and they concatenate **nested-first**:
  - **Nested (recommended).** Write `:::nushell` blocks as **children inside the `:::step` fence**. They are the step's private tasks, auto-scoped to the step name (`<step-name>/<task-name>`), and run in **document order**. Co-locating the tasks with the step teaches the grouping at a glance.
  - **Referenced.** A `tasks: ["shared-a", "shared-b"]` attribute on the `:::step` references shared `:::nushell` blocks defined at stage/chapter level (siblings of the steps). `task: "x"` is single-task sugar for `tasks: ["x"]`. (There is **no** `task_ref:` attribute.)
  - You can mix: nested children run first (doc order), then the `tasks: [...]` refs.
  - Tasks run in authored order and the step **fails together** — the first task failure stops the step (later tasks do not run). A `:::nushell` carrying `role: "directive"` (attached via `for:`) is **flow control, not a task** — see [Directives](./directives.md).
  - Every step that runs tasks needs at least one task; ingest rejects a task-less step (unless it is a pure human-input checkpoint).
- **Other nesting is allowed** but does not create structural relationships -- nested blocks are recorded as children of their parent block.

```
:::stage { name: "Data Collection" }

:::step { name: "Gather Info" }

Collect user information.

:::nushell { name: "process-info" }
def main [] { { payload: {}, signals: {} } }
:::

:::

:::step { name: "Validate" }
Confirm the collected data is complete.
:::

:::
```

The parser uses a stack to track nesting. Each opening `:::directive` pushes onto the stack. Each closing `:::` pops the stack and attaches the completed block as a child of the next block on the stack, or to the top-level result if the stack is empty.

---

## Tag System

Tags classify blocks by audience. They are specified in the `tags` array within the attribute object.

**Valid tags:**

| Tag | Audience |
|-----|----------|
| `agent` | LLM agents and automation consumers |
| `product` | Product managers and non-technical readers |
| `coder` | Developers and technical implementers |
| `auditor` | Compliance and audit reviewers |
| `planner` | Journey planners and workflow designers |

**Untagged blocks** (empty `tags` array or no `tags` attribute) are treated as universal -- they are included in all filtered views.

Tags are used by the render system (`dapple-render`) to filter blocks when generating PDFs or other output formats. Each render profile has a `tag_filter` that selects which tagged blocks appear in the output.

```
:::step { name: "Review Data", tags: [agent, auditor] }
Verify all collected data meets compliance requirements.
:::

:::nushell { name: "validate-fields", tags: [coder] }
def main [] { { payload: { valid: true }, signals: {} } }
:::
```

---

## Special Attributes

Beyond `tags`, several attributes have special meaning across directive types:

| Attribute | Used By | Description |
|-----------|---------|-------------|
| `name` | All types | Identifies the block. Required for most directive types. Links to Fluent message keys. |
| `tags` | All types | Audience classification array. |
| `version` | `stage`, `step`, `nushell` | Semver string for idempotent ingest deduplication. When present, allows the ingest pipeline to skip re-creation of unchanged objects. |
| `ref` | Any directive | Path to shared content (e.g., `"common/validate-output"`). Mutually exclusive with `name`. Indicates the block references shared content from a `_shared/` library rather than defining new content. |
| `slug` | `ref` | The slug of the referenced object. |
| `type` | `ref` | The type of the referenced object (default: `"guide"`). |

**`name` and `ref` are mutually exclusive.** A block cannot have both a `name` attribute and a `ref` attribute. The parser returns a `RefAndNameConflict` error if both are present. Use `name` for blocks that define content; use `ref` for blocks that reference shared content.

---

## Code Block Passthrough

Triple backtick code blocks (` ``` `) inside a directive are treated as opaque content. The parser tracks code block state and does not interpret `:::` lines within code blocks as fence delimiters.

```
:::step { name: "Show Example" }

Here is an example nushell script:

```nushell
:::stage { name: "This Is Not Parsed As A Fence" }
echo "The ::: above is inside a code block and is treated as plain text"
:::
```

:::
```

This allows you to document directive syntax within directive blocks without triggering the parser.

---

## Error Handling

### Unclosed fences

Every opening `:::directive` must have a matching closing `:::`. If the parser reaches the end of a chapter file with an unclosed fence on the stack, it produces an `UnclosedFence` error with the file path and line number of the opening fence.

```
:::stage { name: "Incomplete" }

This stage is never closed.
```

Produces: `UnclosedFence { file: "intro.md", line: 1 }`

### Stray closing fences

A closing `:::` that appears when no fence is open is treated as prose (plain text), not as an error. This is a permissive behavior to avoid breaking documents with minor formatting issues.

### Invalid attributes

If the attribute object cannot be parsed as JSON (even after YAML-like normalization), the parser produces an `InvalidAttributes` error with the file path, line number, and a detail message.

### Missing name attribute

Most directive types require a `name` attribute. If a `nushell`, `brief_pack`, or `inference_profile` block lacks a `name`, the parser emits a warning (not an error) and skips creating the derived object. The block is still stored as a Block entity.

---

## Complete Example

A chapter file demonstrating multiple directive types:

```markdown
# Building a Resume

This chapter walks through the resume building workflow.

:::stage { name: "Data Collection" }

:::step { name: "Personal Info", tags: [agent, product] }

Collect the user's name, email, and contact information.

:::nushell { name: "collect-info", tags: [coder] }
def main [] {
    let info = $in | get context | select name email phone
    { payload: { info: $info }, signals: { fields: 3 } }
}
:::

:::

:::step { name: "Work History", tags: [agent, product] }

Ask the user about their previous work experience.

:::

:::

:::brief_pack { name: "resume-builder-pack", tags: [product] }
:::

:::inference_profile { name: "resume-extractor", tags: [agent] }
:::

:::ref { slug: "formatting-guidelines", type: "guide" }
See the Formatting Guidelines for resume styling conventions.
:::
```

This chapter produces:

- One `stage` block ("Data Collection") with two child `step` blocks
- Two `step` blocks ("Personal Info" with a child `nushell`, and "Work History")
- One `nushell` block that becomes a `TaskDef`
- One `brief_pack` block that becomes a `BriefPackDef`
- One `inference_profile` block that becomes an `InferenceProfileDef`
- One `ref` block that becomes a `RefDef`
- Implicit `prose` blocks for the markdown text between directives
