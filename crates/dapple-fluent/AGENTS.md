### Fluent-Native Architecture

Guides carry **no locale field** — the guide structure is locale-independent. All localizable text lives in `.ftl` files under `locales/`. The pipeline:

1. `dapple-parser` discovers `.ftl` files → parses into `FluentMessageDef` (one per message per locale)
2. `plan_guide()` groups by message key → `FluentMessageSet` (all locales for one key)
3. `materialize_guide()` inserts into `rev.fluent_messages` and `rev.fluent_message_sets`

### Message Key Convention

Fluent message keys use **kebab-case** and correspond to directive `name` attributes:

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

### Fluent Attributes

Fluent attributes (`.key = value`) carry metadata for directives:
- Brief pack blocks: `.card-subtitle`, `.card-category`
- Inference profile blocks: `.description`, `.system-prompt`, `.temperature`, `.max-tokens`
- Stage/step blocks: `.description` (for extended descriptions)

### Term Resolution

Fluent terms (prefixed with `-`) are parsed but **resolved at parse time** per the architecture plan. Term references in message bodies are stored as `{ -term-name }` in the body text. Full runtime Fluent resolution is deferred to the conduit layer.

