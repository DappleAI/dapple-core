# Fluent Localization for Guide Content

> **Audience:** Guide Authors - Prompt Engineers

This chapter covers writing `.ftl` (Fluent) files that provide localized text for guide directives. Every piece of localizable content in a guide -- stage descriptions, step descriptions, brief pack metadata, inference profile system prompts -- lives in Fluent files rather than inline in the markdown source.

This is distinct from the "Fluent Messages" API entity described in [Fluent Messages](./fluent-messages.md), which covers the structured conversation protocol (system/user/assistant/tool roles) used by dapple-prompt-conduit for LLM interactions. This chapter is about the **authoring-time** localization format, not the runtime conversation model.

---

## The `locales/` Directory

Every guide directory may contain a `locales/` subdirectory with one `.ftl` file per locale:

```
my-guide/
  guide.toml
  SUMMARY.md
  chapter-1.md
  chapter-2.md
  locales/
    en.ftl
    pt-BR.ftl
    es.ftl
```

**File naming convention:** Each file is named with an IETF BCP 47 language tag as its stem. The parser derives the locale identifier from the file name:

| File Name | Locale Tag |
|-----------|------------|
| `en.ftl` | `en` |
| `pt-BR.ftl` | `pt-BR` |
| `es.ftl` | `es` |
| `ja.ftl` | `ja` |
| `zh-Hans.ftl` | `zh-Hans` |

If the `locales/` directory does not exist, the parser returns an empty locale map and no Fluent messages are generated. This is valid for guides that contain only inline content (nushell scripts, cross-references).

---

## Message Key Format

Fluent message keys use **kebab-case** and must match the `name` attribute of the directive block they supply content for. This is the binding mechanism between markdown directives and Fluent text.

For example, a stage directive:

```markdown
:::stage { name: "topic-selection" }
:::
```

expects a Fluent message with the key `topic-selection` in each locale file:

```ftl
# locales/en.ftl
topic-selection =
    Guide the user from initial brainstorming through to a focused
    thesis statement.
```

If the parser finds a directive with a `name` attribute but no corresponding Fluent message key, the block's `content` field will be `None` (unless the markdown body contains non-empty fallback text, which is preserved as-is).

---

## Basic Message Syntax

A Fluent message has a key and a body, separated by `=`:

```ftl
message-key =
    The body text goes here. It can span multiple lines as long
    as continuation lines are indented.
```

Single-line messages are also valid:

```ftl
message-key = Short body text here.
```

**Whitespace rules:** Leading and trailing whitespace in the body is trimmed during parsing. Continuation lines must be indented (typically four spaces) relative to the key.

---

## Attribute Syntax

Fluent attributes attach structured metadata to a message. They appear on indented lines below the body, prefixed with a dot:

```ftl
message-key =
    The main body text.
    .description = Extended description of this message
    .some-other-attr = Another attribute value
```

Attributes are key-value pairs where the key starts with `.` and the value follows `=`. Each attribute must be on its own line, indented to the same level as the body continuation.

---

## Common Attributes by Directive Type

Different directive types expect different Fluent attributes. The body text always serves as the primary content for the directive.

### Stage and Step Blocks

| Attribute | Purpose | Example |
|-----------|---------|---------|
| `.description` | Extended description beyond the body | `.description = Collects and validates user input data` |

```ftl
topic-selection =
    Guide the user from initial brainstorming through to a focused
    thesis statement.
    .description = This stage handles the creative exploration phase

build-resume-step =
    Assemble the resume from collected data.
    .description = Runs format and render tasks in sequence
```

### Brief Pack Blocks

| Attribute | Purpose | Example |
|-----------|---------|---------|
| `.card-subtitle` | Subtitle shown on the promotional card | `.card-subtitle = Ready to write an essay?` |
| `.card-category` | Category tag for card display | `.card-category = researcher` |

```ftl
essay-helper-pack =
    The Essay Helper guide walks users through essay writing.
    .card-subtitle = Ready to write an essay?
    .card-category = researcher
```

### Inference Profile Blocks

| Attribute | Purpose | Example |
|-----------|---------|---------|
| `.description` | Human-readable profile description | `.description = Resume data extraction profile` |
| `.system-prompt` | The system prompt sent to the LLM | `.system-prompt = You are a structured data extractor.` |
| `.temperature` | Sampling temperature (as a string) | `.temperature = 0.3` |
| `.max-tokens` | Maximum output tokens (as a string) | `.max-tokens = 2048` |

```ftl
resume-extractor =
    Extract structured resume data from free-form text.
    .description = Resume data extraction profile
    .system-prompt = You are a structured data extractor. Extract name, email, and work history from the provided text. Return valid JSON.
    .temperature = 0.3
    .max-tokens = 2048
```

See [Defining Inference Profiles](./defining-inference-profiles.md) for the full inference profile workflow.

---

## Multi-Locale Example

The power of the Fluent model is that the guide structure (directives, chapters, blocks) is locale-independent. Only the text content varies per locale. Here is a side-by-side example:

**`locales/en.ftl`:**

```ftl
topic-selection =
    Guide the user from initial brainstorming through to a focused
    thesis statement.
    .description = Creative exploration and topic narrowing phase

brainstorm-topics =
    Generate a list of potential essay topics.
    .description = Extended brainstorm description

essay-helper-pack =
    The Essay Helper guide walks users through essay writing.
    .card-subtitle = Ready to write an essay?
    .card-category = researcher
```

**`locales/pt-BR.ftl`:**

```ftl
topic-selection =
    Orientar o usuario desde o brainstorming inicial ate uma
    declaracao de tese focada.
    .description = Fase de exploracao criativa e refinamento do tema

brainstorm-topics =
    Gerar uma lista de topicos potenciais para redacao.
    .description = Descricao estendida do brainstorming

essay-helper-pack =
    O guia Assistente de Redacao orienta os usuarios na escrita.
    .card-subtitle = Pronto para escrever uma redacao?
    .card-category = pesquisador
```

Both files define the same message keys. During ingest, the parser groups them by key into `FluentMessageSet` entries (described below), so all locales for `topic-selection` are stored together.

---

## Terms and Term References

Fluent terms are reusable text fragments defined with a dash (`-`) prefix. They are useful for product names, brand terms, or repeated phrases that should be consistent across messages.

### Defining a Term

```ftl
-product-name = Dapple Prompt
-company = Dapple AI
```

### Referencing a Term

Use `{ -term-name }` (with spaces inside the braces) to reference a term in a message body:

```ftl
welcome-message =
    Welcome to { -product-name }! This guide is brought to you
    by { -company }.
```

### How Terms Are Stored

Terms are parsed but **resolved at parse time**. The parser stores term references in the body text as `{ -term-name }` literal strings. Full Fluent runtime resolution (where `{ -term-name }` is replaced with the term's value) is deferred to the conduit layer. This means the stored body text retains the reference syntax.

The parser skips terms when building `FluentMessageDef` entries -- only messages (without the `-` prefix) produce entries.

---

## How FluentMessageSet Is Built

The transform layer groups all locale-specific message definitions by key into a single `FluentMessageSet` per unique key. This happens in `plan_guide()` during the transform phase.

The data structure:

| Field | Type | Description |
|-------|------|-------------|
| `key` | `String` | The message key (matches directive `name`) |
| `description` | `Option<String>` | Auto-extracted from the first locale's body (up to 200 characters) |
| `bodies` | `HashMap<String, String>` | Locale tag to body text (e.g., `"en" -> "Guide the user..."`) |
| `attributes` | `HashMap<String, HashMap<String, String>>` | Locale tag to attribute map (e.g., `"en" -> {"description": "..."}`) |

**Example result for the `topic-selection` key:**

```
FluentMessageSet {
    key: "topic-selection",
    description: Some("Guide the user from initial brainstorming..."),
    bodies: {
        "en": "Guide the user from initial brainstorming through to a focused thesis statement.",
        "pt-BR": "Orientar o usuario desde o brainstorming inicial ate uma declaracao de tese focada.",
    },
    attributes: {
        "en": { "description": "Creative exploration and topic narrowing phase" },
        "pt-BR": { "description": "Fase de exploracao criativa e refinamento do tema" },
    },
}
```

During materialization, each `FluentMessageSet` becomes a `rev.fluent_messages` path with one version per locale, or a single version with all locales encoded -- depending on the ingest handler's strategy.

---

## Inline vs Fluent-Backed Blocks

Not all directive types use Fluent for their content. The rule is straightforward:

| Directive | Content Source | Rationale |
|-----------|---------------|-----------|
| `prose` | Fluent-backed | Narrative text varies by locale |
| `stage` | Fluent-backed | Stage descriptions vary by locale |
| `step` | Fluent-backed | Step descriptions vary by locale |
| `brief_pack` | Fluent-backed | Pack content and card metadata vary by locale |
| `inference_profile` | Fluent-backed | System prompts and descriptions vary by locale |
| `block` | Fluent-backed | Generic block with localizable content |
| `nushell` | Inline (`content = Some(...)`) | Code is not localized |
| `ref` | Inline (`content = Some(...)`) | Structural cross-reference, not localized |

In the `IngestPlan`, Fluent-backed blocks with empty markdown bodies have `content: None`. Inline blocks always retain their `content: Some(...)`. If a Fluent-backed block has non-empty markdown text, that text is preserved as a fallback value.

---

## Brief inputs and dynamic choices

A `:::brief-input`'s **label / help / placeholder** are Fluent-backed (keyed by the input's `name`), so localize them like any other directive text.

Its **choices** split by where they come from:

- **Static choices** authored on the input are localizable Fluent text.
- **Dynamic choices** — the tiles a **keep-N** (`cardinality: "multi"`) or single dynamic pick draws from a `candidate_source` slot — are **produced by the generate task at runtime and are NOT localized via `.ftl`**. The `label` / `description` / `value` on each tile are whatever the task put in its `{ payload: { choices: [...] } }`.

  To localize dynamic tiles, the **generate task must emit locale-appropriate text** (e.g. branch on the journey locale, or run a localized extraction). There is no `.ftl` binding for runtime-generated choice tiles. See [Writing Nushell Tasks → keep-N](./writing-nushell-tasks.md#6-tasks-that-feed-or-consume-keep-n-picks) and [Value-Slot Overlays → Multi-select picks](./value-slot-overlays.md#multi-select-picks-keep-n--curate-a-subset).

---

## Fluent Localization vs Fluent Messages API

These are two separate systems that share the word "Fluent":

| Concept | What It Is | Where It Lives |
|---------|-----------|----------------|
| **Fluent Localization** (this chapter) | `.ftl` files providing localized text for guide directives | `locales/` directory in guide source, materialized to `rev.fluent_messages` |
| **Fluent Messages API** | Structured conversation protocol (system/user/assistant/tool roles) for LLM interactions | dapple-prompt-conduit runtime, documented in [Fluent Messages](./fluent-messages.md) |

The naming overlap is historical. When working with guide content, you are working with **Fluent Localization**. When working with LLM conversation turns, you are working with the **Fluent Messages API**.

---

## Best Practices

1. **Keep message keys stable** -- Renaming a key breaks the binding between the directive and its Fluent content. If you must rename, update both the directive's `name` attribute and all `.ftl` files.

2. **Provide all locales for every key** -- If `en.ftl` defines `topic-selection` but `pt-BR.ftl` does not, the Portuguese locale will have no body for that directive. The ingest pipeline will not fail, but the missing locale will produce incomplete content.

3. **Use attributes for metadata, not the body** -- The body is the primary human-readable content. Structured data (temperatures, token limits, categories) belongs in attributes where it can be parsed mechanically.

4. **Keep attribute values simple** -- Fluent attributes are single-line strings. Complex values should be JSON-encoded if necessary, but prefer flat strings where possible.

5. **Match every directive to a message** -- Every `:::stage`, `:::step`, `:::brief_pack`, and `:::inference_profile` block with a `name` attribute should have a corresponding message key in every locale file.

6. **Use terms for repeated text** -- Product names, brand terms, and other constants should be defined as terms (`-term-name = value`) and referenced consistently across messages.
