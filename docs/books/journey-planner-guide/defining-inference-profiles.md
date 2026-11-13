# Defining Inference Profiles

> **Audience:** Guide Authors - Prompt Engineers

Inference profiles configure how LLM prompts within a guide are executed -- which model to use, what system prompt to send, and what sampling parameters to apply. This chapter covers authoring inference profiles in guide content. For runtime model resolution and backend routing, see [dapple-prompt-conduit](./dapple-prompt-conduit.md).

---

## What Is an Inference Profile?

An inference profile is a named configuration object that binds a prompt to a specific model configuration. When a task within a playbook needs to call an LLM, it references an inference profile by name. The profile determines:

- **System prompt** -- The instructions sent to the model before user input
- **Model selection** -- Which lineage, provider, tier, and release to use
- **Sampling parameters** -- Temperature, max tokens, and other generation controls
- **Priority** -- When multiple model releases are configured, which to prefer

Profiles are defined in guide markdown as `:::inference_profile` directive blocks, with their localizable content (system prompt, description) supplied by Fluent `.ftl` files.

---

## Directive Syntax

An inference profile block uses the `:::inference_profile` directive with a `name` attribute:

```markdown
:::inference_profile { name: "resume-extractor" }
:::
```

The block body in markdown is typically empty because the content lives in Fluent files. If you include markdown text in the body, it is preserved as fallback content but the Fluent message takes precedence during materialization.

### Attribute Reference

| Attribute | Required | Description |
|-----------|----------|-------------|
| `name` | Yes | Kebab-case identifier; must match a Fluent message key |
| `tags` | No | Audience tags (e.g., `[agent, product]`) |
| `version` | No | Semantic version for idempotent ingest dedup |

```markdown
:::inference_profile { name: "resume-extractor", version: "1.0.0", tags: [agent] }
:::
```

---

## Fluent Configuration

The substantive configuration for an inference profile lives in the `.ftl` files under `locales/`. The Fluent message key must match the directive's `name` attribute.

### Message Structure

The message **body** serves as the primary description of the profile. Attributes carry the operational configuration:

| Attribute | Purpose | Example Value |
|-----------|---------|---------------|
| `.description` | Human-readable profile description | `Resume data extraction profile` |
| `.system-prompt` | System prompt sent to the model | `You are a structured data extractor...` |
| `.temperature` | Sampling temperature (string-encoded float) | `0.3` |
| `.max-tokens` | Maximum output token count (string-encoded integer) | `2048` |
| `.lineage` | Model lineage for automatic config wiring | `claude-4-6` |
| `.tier` | Model tier within the lineage | `sonnet` |
| `.release` | Model release version | `20250929` |

### Example

**Markdown directive:**

```markdown
:::inference_profile { name: "resume-extractor" }
:::
```

**`locales/en.ftl`:**

```ftl
resume-extractor =
    Extract structured resume data from free-form text.
    .description = Resume data extraction profile
    .system-prompt = You are a structured data extractor. Extract name, email, and work history from the provided text. Return valid JSON.
    .temperature = 0.3
    .max-tokens = 2048
    .lineage = claude-4-6
    .tier = sonnet
    .release = 20250929
```

**`locales/pt-BR.ftl`:**

```ftl
resume-extractor =
    Extrair dados estruturados de curriculo a partir de texto livre.
    .description = Perfil de extracao de dados de curriculo
    .system-prompt = Voce e um extrator de dados estruturados. Extraia nome, email e historico profissional do texto fornecido. Retorne JSON valido.
    .temperature = 0.3
    .max-tokens = 2048
    .lineage = claude-4-6
    .tier = sonnet
    .release = 20250929
```

Note that `.temperature`, `.max-tokens`, `.lineage`, `.tier`, and `.release` are typically the same across locales (they are operational parameters, not localizable text), but they must be present in each locale file because attributes are stored per-locale in the `FluentMessageSet`.

The `.lineage`, `.tier`, and `.release` attributes enable automatic model wiring during ingest. When present, the materialization pipeline resolves the corresponding `model_release_id` from `rev.model_releases` and creates `inference_profile_configs` entries linking the profile to the resolved model. If these attributes are omitted, the profile is created without automatic config wiring.

---

## How Profiles Are Materialized

During guide ingest, the transform layer extracts an `InferenceProfileDef` from each `:::inference_profile` block:

```
InferenceProfileDef {
    name: "resume-extractor",
    source_block_index: BlockIndex { chapter_index: 2, block_index: 0 },
}
```

The profile's description, system prompt, temperature, and max tokens are not stored on the `InferenceProfileDef` itself. They are carried by the corresponding `FluentMessageSet` entry (keyed by `"resume-extractor"`) and materialized as Fluent message attributes during the database transaction.

The materialization handler performs full wiring in four steps:

1. **Creates or looks up the inference profile** entity by name (name-based deduplication)
2. **Creates a Prompt entity** linked to the profile's FluentMessage via `rev.prompt_fluent_messages`
3. **Creates task-prompt links** — a cross-product of all tasks and all prompts via `rev.task_prompts`
4. **Resolves model config and creates `rev.inference_profile_configs`** — extracts `.lineage`, `.tier`, and `.release` from the FluentMessageSet attributes, resolves the `model_release_id` from `rev.model_releases`, and creates the config entry linking the profile to the prompt and model release

---

## Relationship to the Model Taxonomy

Inference profiles sit at the intersection of guide authoring and the model taxonomy. The taxonomy defines the available models:

```
Provider (e.g., "anthropic", "self-hosted")
  -> Lineage (e.g., "claude-3-5", "claude-4-6", "flan-t5")
    -> Model Release (lineage + provider + tier + release)
      -> Model Variant (optional fine-tuned branch)
```

An inference profile references one or more model releases through `rev.inference_profile_configs`. Each config entry specifies:

| Field | Description |
|-------|-------------|
| `prompt_path_object_id` | The prompt this config applies to |
| `model_release_id` | The model release to use (mutually exclusive with `model_variant_id`) |
| `model_variant_id` | A fine-tuned variant to use (mutually exclusive with `model_release_id`) |
| `priority` | Integer priority; lower values are preferred |

### Release Chain Resolution

At runtime, the conduit resolves a model config by building a fallback chain from most specific to least specific:

```
@provider:lineage/tier:release   (most specific)
@provider:lineage/tier
lineage/tier:release
lineage/tier
lineage                          (least specific)
```

The first chain entry that matches an available backend is selected for execution.

---

## The Priority System

When an inference profile has multiple `inference_profile_configs` entries, the `priority` field determines preference order. This enables fallback strategies:

```
Config 1: model_release = claude-3-5-sonnet, priority = 1
Config 2: model_release = claude-3-haiku,    priority = 10
```

The conduit tries priority 1 first. If that model is unavailable (provider down, rate limited), it falls back to priority 10. Lower priority numbers are preferred.

Use cases for multiple configs:

- **Cost optimization** -- Prefer a cheaper model, fall back to a more capable one
- **Availability** -- Prefer a self-hosted model, fall back to a cloud provider
- **A/B testing** -- Route different percentages of traffic to different models by manipulating priorities at the journey level

---

## When to Use Multiple Profiles

A guide typically defines one inference profile per distinct LLM interaction pattern. Consider separate profiles when:

- **Different system prompts** -- A data extraction task and a summarization task need different instructions
- **Different sampling parameters** -- Creative writing (high temperature) vs structured extraction (low temperature)
- **Different model requirements** -- A task needs a vision model while another needs a text-only model
- **Different cost profiles** -- A quick classification can use a smaller model while a complex analysis needs a larger one

### Example: Multiple Profiles in One Guide

```markdown
:::inference_profile { name: "resume-extractor" }
:::

:::inference_profile { name: "cover-letter-writer" }
:::

:::inference_profile { name: "job-match-scorer" }
:::
```

```ftl
# locales/en.ftl

resume-extractor =
    Extract structured resume data from free-form text.
    .description = Structured data extraction from resumes
    .system-prompt = You are a structured data extractor. Extract name, email, education, and work history. Return valid JSON.
    .temperature = 0.2
    .max-tokens = 2048

cover-letter-writer =
    Generate a professional cover letter from resume data and job description.
    .description = Cover letter generation from structured inputs
    .system-prompt = You are a professional cover letter writer. Given structured resume data and a job description, write a compelling cover letter that highlights relevant experience. Use a professional but warm tone.
    .temperature = 0.7
    .max-tokens = 4096

job-match-scorer =
    Score how well a resume matches a job description.
    .description = Resume-to-job compatibility scoring
    .system-prompt = You are an HR analyst. Given a resume and job description, return a JSON object with: overall_score (0-100), strengths (array of strings), gaps (array of strings), and recommendation (string).
    .temperature = 0.1
    .max-tokens = 1024
```

Each profile is independent. Steps or tasks within the guide's derived playbook reference profiles by name to select the appropriate LLM configuration.

---

## Linking Profiles to Steps

Inference profiles are linked to playbook execution through `rev.inference_profile_configs`, which maps a prompt path to a model release within the context of a profile. The journey-level binding happens through `rev.journey_inference_profiles`, which associates profiles with specific journeys.

This separation means:

- **Authors** define profiles in guides (what model settings to use)
- **Operators** bind profiles to journeys (which profiles are active for a given user session)
- **The conduit** resolves the active profile at runtime and routes to the appropriate backend

For the full runtime resolution flow, see [dapple-prompt-conduit](./dapple-prompt-conduit.md).

---

## Best Practices

1. **Use descriptive profile names** -- `resume-extractor` is better than `profile-1`. The name appears in logs, audit trails, and configuration UIs.

2. **Keep system prompts focused** -- Each profile should have a single, clear instruction set. If you find a system prompt growing beyond a paragraph, consider whether the task should be split.

3. **Set temperature deliberately** -- Low values (0.1-0.3) for structured extraction and classification. Medium values (0.4-0.6) for balanced tasks. Higher values (0.7+) for creative generation. Document your reasoning in the `.description` attribute.

4. **Set max-tokens conservatively** -- Over-provisioning wastes compute budget. Estimate the expected output size and add a reasonable buffer. For structured JSON output, the token count is usually predictable.

5. **Version your profiles** -- When changing a system prompt or sampling parameters, use the `version` attribute on the directive to enable idempotent ingest. The ingest pipeline uses version matching to decide whether to create a new version or skip a duplicate.

6. **Match locale attribute values when appropriate** -- While `.system-prompt` should be translated for multilingual deployments, `.temperature` and `.max-tokens` are typically identical across locales. Keep them consistent to avoid unexpected behavior differences between locales.

7. **One profile per interaction pattern** -- Resist the temptation to create a single "general purpose" profile. Distinct tasks benefit from distinct configurations, and the priority system handles fallback gracefully.
