# Model Taxonomy

> **Audience:** Prompt Engineers · Product · Agents · Developers

This document describes the model taxonomy - a structured way to think about AI models and how to configure them for prompt execution.

---

<!-- ANCHOR: hierarchy -->
## Hierarchy

The model taxonomy organizes AI models in a hierarchy that separates concerns:

```
Provider → Lineage → Tier → Release → Variant
```

| Level | What it represents | Example |
|-------|-------------------|---------|
| **Provider** | Where the model is hosted | Anthropic, Self-Hosted, NVIDIA NIM |
| **Lineage** | Prompting paradigm | claude-3-5, vits, flan-t5 |
| **Tier** | Capability level | sonnet, opus, ljspeech |
| **Release** | Version snapshot | 20241022, v0.1 |
| **Variant** | Fine-tune branch | acme-customer-v1 |

This hierarchy enables the same Playbook to run with different models by simply changing the InferenceProfile attached to the Journey.
<!-- ANCHOR_END: hierarchy -->

---

<!-- ANCHOR: lineage -->
## Lineage: The Prompting Language

Think of a Lineage as a "language" that a model family speaks. Models in the same lineage respond well to prompts structured the same way.

**Examples:**
- **Claude models** understand pseudo-XML structured prompts with `<context>`, `<instructions>` tags
- **GPT models** prefer pseudo-JSON structures with explicit role definitions
- **VITS models** expect speaker configuration with `speaker_id` and `language` parameters

When you write a prompt template for the "claude-3-5" lineage, you're writing in Claude's language. That template works whether you're talking to Sonnet, Opus, or Haiku - they all understand the same prompting patterns.

**Why this matters:**
- Switching providers doesn't require new templates if the Lineage stays the same
- A/B testing between tiers (sonnet vs opus) is straightforward
- Model upgrades within a lineage preserve your prompt investment
<!-- ANCHOR_END: lineage -->

---

<!-- ANCHOR: provider -->
## Provider

A Provider is where a model is hosted and billed. The same Lineage can exist at multiple providers:

| Provider | Description | Use Case |
|----------|-------------|----------|
| `anthropic` | Anthropic's hosted API | Production Claude models |
| `openai` | OpenAI's hosted API | GPT model family |
| `self-hosted` | Your own infrastructure | Privacy, cost control |
| `nvidia-nim` | NVIDIA's inference microservices | Optimized inference |

**Provider on model_releases:** The provider is associated with each model release, not the lineage. This allows the same model (e.g., Mistral-7B) to be available from multiple providers.
<!-- ANCHOR_END: provider -->

---

<!-- ANCHOR: tier -->
## Tier

A Tier represents the capability level within a lineage. Think of tiers like dialects - they speak the same language but with different capabilities and nuances.

**Examples:**
- Claude: `sonnet` (balanced), `opus` (highest capability), `haiku` (fastest)
- VITS: `ljspeech` (English single speaker), `vctk` (multi-speaker)
- FLAN-T5: `base` (small), `large` (bigger context)

**Selecting tiers:** Choose based on your task requirements:
- **Speed-critical tasks** → Smaller tiers (haiku, base)
- **Quality-critical tasks** → Larger tiers (opus, large)
- **Cost-sensitive tasks** → Balanced tiers (sonnet)
<!-- ANCHOR_END: tier -->

---

<!-- ANCHOR: release -->
## Release

A Release is a specific version snapshot from the provider. Examples:
- `20241022` - Claude model dated version
- `v0.1` - Semantic version
- `NULL` - Unversioned (uses provider's default)

Releases capture "dialect" changes within a tier. A release chain provides fallback resolution:

```
["@anthropic:claude-3-5/sonnet:20241022",  -- Exact provider+version
 "@anthropic:claude-3-5/sonnet",            -- Provider+tier
 "claude-3-5/sonnet:20241022",              -- Shared version
 "claude-3-5/sonnet",                       -- Shared tier
 "claude-3-5"]                              -- Lineage fallback
```
<!-- ANCHOR_END: release -->

---

<!-- ANCHOR: variant -->
## Variant

A Variant is a fine-tuned model that branches from a specific Release. Variants are useful for:
- Custom fine-tunes trained on your domain data
- A/B testing prompt strategies
- Customer-specific adaptations

**Key property:** Variants inherit their provider from their base release. A variant of `claude-3-5/sonnet:20241022` at Anthropic is also hosted at Anthropic.

**Variant resolution:** When executing with a variant, the system first checks for variant-specific templates, then falls back to the base release's templates.
<!-- ANCHOR_END: variant -->

---

<!-- ANCHOR: inference-profile -->
## InferenceProfile

An InferenceProfile maps prompts to models at Journey creation time. This is the key to making Playbooks model-agnostic.

**Three-Part Journey Composition:**

```
Journey = Playbook + BriefPacks + InferenceProfiles
```

- **Playbook** - What tasks to run
- **BriefPacks** - What forms users fill out
- **InferenceProfiles** - Which models to use

**Benefits:**
1. Same Playbook works with different InferenceProfiles
2. A/B test model quality without changing workflow logic
3. Self-hosted vs API-hosted is a deployment choice, not a code change

**Multiple options per prompt:** An InferenceProfile can specify multiple models for the same prompt, with priority ordering. The Conduit selects from available options based on:
- Provider availability
- Health status
- Credit/quota
- Provider-specific constraints
<!-- ANCHOR_END: inference-profile -->

---

<!-- ANCHOR: collaboration -->
## The Collaboration Model

This architecture mirrors how human languages and collaboration work:

| Human Languages | Model Taxonomy |
|-----------------|----------------|
| Portuguese | Lineage (claude-3-5) |
| ├── Brazilian PT | ├── Tier (sonnet) |
| ├── European PT | ├── Tier (opus) |
| └── Dialects | └── Provider variants |

**Separation of concerns:**
- **Playbook authors** focus on workflow logic
- **Prompt authors** focus on communication style
- **Infrastructure teams** focus on model deployment
- Each role works in their "language"

**Agent-to-model communication:**
- AI agents authoring prompts learn the "dialect" of a Lineage
- Same agent can write for multiple lineages
- Templates are the "translation layer"
<!-- ANCHOR_END: collaboration -->

---

## Quick Reference

### Release Chain Format

The release chain format uses optional `@provider` prefix:

```
@provider:lineage/tier:release   -- Provider-specific exact match
@provider:lineage/tier           -- Provider-specific tier match
lineage/tier:release             -- Shared exact match
lineage/tier                     -- Shared tier match
lineage                          -- Lineage fallback
```

### Common Lineages

| Lineage | Model Family | Use Case |
|---------|--------------|----------|
| `claude-3-5` | Claude 3.5 | General LLM tasks |
| `gpt-4` | GPT-4 | General LLM tasks |
| `mistral` | Mistral | Open-source LLM |
| `vits` | VITS TTS | Text-to-speech |
| `flan-t5` | FLAN-T5 | Summarization |
| `modernbert` | ModernBERT | Masked language modeling |

### Database Tables

| Table | Purpose |
|-------|---------|
| `rev.providers` | API/hosting endpoints |
| `rev.lineages` | Prompting paradigms |
| `rev.model_releases` | Specific models at providers |
| `rev.model_variants` | Fine-tuned branches |
| `rev.inference_profiles` | Journey-level model config |
| `rev.inference_profile_configs` | Prompt-to-model mappings |
| `rev.journey_inference_profiles` | Junction table |
