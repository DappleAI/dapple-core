# Changes

> **:robot: Agent Note:** Skip this chapter. Changes documentation is for human readers tracking version-to-version differences. Agents should read the actual content chapters directly.

This chapter helps human readers understand what changed between versions so you don't have to reread the entire guide. Each version links to the new or updated sections.

---

## [Version 0.3.6](#version-036) (Current)

**Released:** 2026-08-03

The `:::stage` directive gains an optional authored `icon`.

### What Changed

**`:::stage` attribute table updated ([directive-blocks.md](./directive-blocks.md)):**
- New optional **`icon`** attribute — up to five icon names, in your order,
  distinguishing a stage from its siblings *within* a `kind`. A fifteen-stage
  guide can honestly be nine `analyze` panes, and `kind` alone draws nine
  identical discs; this is the field that tells them apart.
- Unlike `kind` the vocabulary is **open**: nothing is validated, an
  unrecognized name is stored verbatim and produces **no warning**, and a
  drawing client simply falls back to the `kind` glyph. The names belong to
  whichever client draws them (dapple-ui curates ~150 lucide names), so authoring
  one before that client knows it is normal.
- Accepts an array or a bare string; hyphenated names need no quoting. A client
  reads the first five — authoring more warns at ingest but stores all of them.
- Adding, changing, **or reordering** icons re-versions the stage on re-ingest
  (it folds into the content hash, and order decides which five are read);
  stages without one keep their existing hashes, so an icon-less guide
  re-ingests unchanged.
- New **[Stage icon names](./directive-blocks.md#stage-icon-names)** section.

---

## [Version 0.3.5](#version-035)

**Released:** 2026-08-03

The `:::stage` directive gains an optional authored `kind`.

### What Changed

**`:::stage` attribute table updated ([directive-blocks.md](./directive-blocks.md)):**
- New optional **`kind`** attribute — a closed vocabulary (`intake`, `fetch`,
  `analyze`, `decide`, `review`, `compare`, `assemble`, `deliver`) naming what
  kind of pane the stage is. Clients render stage icons/labels from it instead
  of guessing by ordinal. An unrecognized value produces an ingest warning and
  is stored as absent. Adding or changing `kind` re-versions the stage on
  re-ingest (it folds into the content hash); stages without one keep their
  existing hashes, so a kindless guide re-ingests unchanged.

---

## [Version 0.3.4](#version-034)

**Released:** 2026-06-13

Adds author-facing troubleshooting for the most common "my guide looks stuck"
report: a task succeeds but its result never surfaces because the step has no
computed `:::brief-output` to bind the payload into a value slot.

### What Changed

**Testing & Debugging updated ([testing-debugging.md](./testing-debugging.md)):**
- New section **"A task succeeds but its result never appears"** — symptom,
  how to rule out an LLM hang (and why the raw `running` attempt status is a red
  herring), the task-payload-vs-journey-value root cause, the two shapes
  (no computed slot vs. declared-but-unproduced), and the fix.
- "Common Issues" table gains a row pointing at the new section.

**Computed `:::brief-output` examples corrected
([value-slot-overlays.md](./value-slot-overlays.md),
[directive-blocks.md](./directive-blocks.md)):**
- Both books previously showed an **unnamed** `:::brief-output` nested under a
  step — a shape the parser silently drops (it never materializes). Replaced with
  the working contract: a computed output must be **named**, **listed in its
  `:::brief`'s `units`**, and have its **slot exposed on the producing step**.
  The `name` attribute is now documented as required (unnamed → dropped at parse).

---

## [Version 0.3.3](#version-033)

**Released:** 2026-05-29

Adds value-slot overlays — how a stage declares the slots it needs, which is
what enables per-stage completion ("is this stage done?") and focuses the
operator form.

### What Changed

**New chapter ([value-slot-overlays.md](./value-slot-overlays.md)):**
- Authoring `expose` / `mask` on `:::stage` / `:::step`, the stage→step→task cascade
- Why overlays drive completion gating, the operator form, and agent context
- **Remediation for existing guides** (which have no overlays ⇒ no gating): edit
  the source to add `expose`, then re-ingest. A backfill alone cannot add them.
- Re-ingesting overlays forks a new version, so in-flight journeys are unaffected.

**Directive Blocks updated ([directive-blocks.md](./directive-blocks.md)):**
- `stage` / `step` attribute tables document the `expose` / `mask` attributes.

---

## [Version 0.3.2](#version-032)

**Released:** 2026-02-19

This release updates documentation to reflect the Guide Lifecycle Audit changes: full prompt entity wiring during ingest, Claude 4.6 model taxonomy, and lineage-aware conduit routing.

### What Changed

**Ingest Pipeline updated ([ingest-pipeline.md](./ingest-pipeline.md)):**
- Materialization diagram now shows full 6-step derived object pipeline (tasks, inference profiles, prompts, task-prompts, inference profile configs, playbook)
- Prompt entity creation, task-prompt cross-product linking, and inference profile config wiring documented
- Response schema includes `prompt_object_ids`

**Inference Profiles updated ([defining-inference-profiles.md](./defining-inference-profiles.md)):**
- New Fluent attributes: `.lineage`, `.tier`, `.release` for model selection during ingest
- Materialization section reflects full 4-step wiring (profile, prompt, task-prompts, configs)
- Model taxonomy examples include Claude 4.6

**Conduit Routing updated ([dapple-prompt-conduit.md](./dapple-prompt-conduit.md)):**
- LLM Backends table corrected (uses lineage keys `claude-3-5`, `claude-4-6`, not provider keys)
- Debug example updated to current model reference

### Quick Links for 0.3.2

If upgrading from 0.3.1:
1. [Ingest Pipeline - Step 6](./ingest-pipeline.md#step-6-materialize-derived-objects) -- Full derived object pipeline
2. [Defining Inference Profiles - Fluent Configuration](./defining-inference-profiles.md#fluent-configuration) -- New model selection attributes
3. [Prompt Execution - LLM Backends](./dapple-prompt-conduit.md#llm-backends) -- Corrected lineage keys

---

## [Version 0.3.1](#version-031)

**Released:** 2026-02-18

This release cleanly separates the Journey Planner Guide (guide authoring and workflow design) from the API Consumer Guide (programmatic API consumption). Content that was duplicated or misplaced between the two guides has been relocated to its proper home.

### What Changed

**Removed from this guide (moved to API Consumer Guide):**
- Part III "Consuming the Platform" section removed entirely (authentication, SDK, API patterns, runtime operations, API cookbook)
- `dapple-sdk.md` moved to API Consumer Guide as `sdk-overview.md`
- `resume-walkthrough.md` (Nushell-first walkthrough) removed
- API call examples from `directives.md` moved to consumer guide's `directives.md`

**Restructured:**
- Part III renamed "Workflow Design" -- groups directives, Nushell tasks, custom commands
- Part IV renamed "Runtime for Authors" -- same chapters with trimmed implementation internals
- `dapple-prompt-conduit.md` trimmed: removed Rust struct definitions, backend selection code, overlay service internals
- `dapple-prompt-flow.md` trimmed: removed StepEngine architecture diagram, parse cache implementation, execution flow internals

**New content:**
- Guide Cards section added to [guide.toml Reference](./guide-toml-reference.md) covering `[card]` authoring, Fluent card attributes, and tag-based rendering
- Tarball creation guidance added to [Ingest Pipeline](./ingest-pipeline.md) with directory layout and common structural errors
- Cross-references added between guides for seamless navigation

### Quick Links for 0.3.1

If upgrading from 0.3.0:
1. [guide.toml Reference - Guide Cards](./guide-toml-reference.md#guide-cards) -- New card authoring section
2. [Ingest Pipeline - Creating a Tarball](./ingest-pipeline.md#creating-a-tarball) -- Tarball creation guidance
3. [API Consumer Guide](../api-consumer-guide/preface.md) -- All API consumption content is now here

---

## [Version 0.3.0](#version-030)

**Released:** 2026-02-18

This release is a comprehensive restructure from API-centric to guide-centric documentation, organized around the guide lifecycle: Author, Ingest, Consume, Execute.

### What's New

**Guide authoring documentation (Part II):**
- New [Guide Directory Structure](./guide-directory-structure.md) -- anatomy of a guide directory
- New [guide.toml Reference](./guide-toml-reference.md) -- complete metadata file reference
- New [Directive Blocks](./directive-blocks.md) -- `:::directive { attrs }` syntax for all 10 directive types
- New [Fluent Localization](./fluent-localization.md) -- writing `.ftl` files for localized content
- New [Defining Inference Profiles](./defining-inference-profiles.md) -- LLM configuration via directives
- New [Shared Content](./shared-content.md) -- cross-guide content sharing
- New [Build a Resume -- Guide Authoring Walkthrough](./resume-guide-walkthrough.md) -- end-to-end guide authoring tutorial

**SDK documentation (Part III):**
- New [The Dapple SDK](./dapple-sdk.md) -- typed Rust client for all API resources

**Execution and runtime (Part IV):**
- New [Ingest Pipeline](./ingest-pipeline.md) -- how guide tarballs are parsed, transformed, and materialized

**Examples:**
- New [Simple Guide Directory](./examples/simple-guide-directory.md) -- minimal complete guide
- New [Resume Guide (Complete)](./examples/resume-guide.md) -- full guide with all directive types

**Restructured:**
- Preface updated to v0.3.0 with guide-first orientation
- Introduction rewritten around guide lifecycle
- Concepts expanded with guide lifecycle, IngestPlan, and derived objects
- Versioning updated with guide.toml version field and POST/PATCH ingest
- AGENTS.md rewritten with guide authoring and SDK navigation
- Nushell Task Template renamed to Writing Nushell Tasks and reframed for inline `:::nushell` blocks
- Testing & Debugging updated with dry-run validation and common ingest errors

**Removed (merged into new chapters):**
- `brief-pack-manifest.md` -- merged into `directive-blocks.md`
- `adding-prompts.md` -- replaced by `defining-inference-profiles.md`
- `composing-steps-stages.md` -- split into `directive-blocks.md` and `dapple-prompt-flow.md`
- `fluent-messages.md` -- split: localization into `fluent-localization.md`, LLM protocol into `dapple-prompt-conduit.md`
- `prompt-authoring.md` -- replaced by `defining-inference-profiles.md`
- `cookbook.md` -- merged into `api-cookbook.md`
- `structuring-guide-content.md` -- replaced by `guide-directory-structure.md`

### Breaking Changes

- Chapter numbering and file names have changed significantly
- Old links to removed chapters will need updating
- `brief-pack-manifest.md`, `composing-steps-stages.md`, `fluent-messages.md` no longer exist

### Quick Links for 0.3.0

If upgrading from 0.2.0:
1. [Guide Directory Structure](./guide-directory-structure.md) -- start here for guide authoring
2. [The Dapple SDK](./dapple-sdk.md) -- new typed client documentation
3. [Ingest Pipeline](./ingest-pipeline.md) -- understand the ingest flow
4. [Directive Blocks](./directive-blocks.md) -- the `:::directive` syntax reference

---

## [Version 0.0.2](#version-002)

**Released:** 2026-01-22

This release focuses on authentication documentation improvements and terminology standardization.

### What's New

**Authentication terminology standardized:**
- Introduced **"Opaque Personal Access Token (PAT)"** terminology to distinguish from JWTs
- Added industry comparisons (GitHub PATs, GitLab PATs, Stripe keys) for clarity
- See: [Opaque Personal Access Tokens (PAT)](./authentication.md#opaque-personal-access-tokens-pat)

**API token documentation corrected to match implementation:**
- Token creation uses `expires_in_secs` (numeric seconds), not `expires_at`
- Token revocation uses `DELETE` endpoint (append-only pattern), not `PATCH`
- Token prefix is `dpl_` (47 chars total, 256 bits entropy)
- See: [Creating an API Token](./authentication.md#creating-an-api-token)

**Agent-specific callout notes added throughout:**
- `:robot: API Consumer Agent Note` - Token detection and rate limits
- `:robot: Token Management Agent Note` - Creation and revocation details
- `:robot: Implementation Agent Note` - Storage and validation internals
- `:robot: External Service Agent Note` - Ethereum auth requirements
- `:robot: Journey Orchestration Agent Note` - Programmatic journey creation
- See: [Authentication chapter](./authentication.md), [API Patterns](./api-patterns.md), [Runtime Operations](./runtime-operations.md)

**Glossary expanded:**
- New "Authentication & API Access" section with precise definitions
- Terms: Opaque PAT, JWT, Token Prefix, Token Hash, Ethereum-Signed Auth Token
- See: [Glossary - Authentication & API Access](./glossary.md#authentication--api-access)

### Quick Links for 0.0.2

If you're upgrading from 0.0.1, read these sections:

1. [Opaque Personal Access Tokens (PAT)](./authentication.md#opaque-personal-access-tokens-pat) - New terminology explanation
2. [Token Format](./authentication.md#token-format) - Updated with correct `dpl_` prefix details
3. [Comparison Table](./authentication.md#comparison-table) - Updated with Token Type and Validation columns
4. [Glossary - Authentication](./glossary.md#authentication--api-access) - New authentication terms

---

## [Version 0.0.1](#version-001)

**Released:** 2026-01-20

Initial release of the Journey Planners Guide with API Consumer Guide integration.

### What's Included

**Core documentation (Chapters 00-13):**
- Introduction, glossary, and concepts
- Nushell task templates and composing steps/stages
- Versioning patterns
- Resume builder walkthrough
- Brief pack manifests
- Testing and debugging
- dapple-prompt-flow runtime
- Custom Nushell commands
- Prompt authoring and dapple-prompt-conduit
- Fluent messages

**API Consumer Guide (Chapters 14-17):**
- [Authentication](./authentication.md) - Three authentication methods
- [API Patterns & Reference](./api-patterns.md) - Batch semantics, endpoint reference
- [Runtime Operations](./runtime-operations.md) - Journey-centric workflows
- [API Cookbook](./api-cookbook.md) - Production-ready recipes

**Reference materials:**
- Cookbook with basic recipes
- Examples section with practical templates
- Appendix

### Known Issues (Fixed in 0.0.2)

- API token documentation showed incorrect request/response format
- Token prefix incorrectly shown as `dpt_` instead of `dpl_`
- Revocation endpoint incorrectly shown as `PATCH` instead of `DELETE`
- Missing agent-specific callout notes

---

## [Version 0.0.3](#version-003)

**Released:** 2026-01-29

This release adds comprehensive documentation for delegated service tokens and JWT refresh patterns.

### What's New

**Service token delegation documented:**
- New "Delegated Tokens" section explaining how services act on behalf of users
- Request schema updated with optional `auth_user_id` parameter
- JWT claims comparison between service and delegated tokens
- See: [Service Tokens - Delegated Tokens](../_foundation/authentication/service-tokens.md#delegated-tokens-acting-on-behalf-of-users)

**Token expiry and refresh guidance:**
- New "Token Expiry & Refresh" section with lifetime summary table
- TypeScript example for token refresh pattern
- Troubleshooting guide for unexpected 401 errors
- See: [Service Tokens - Token Expiry & Refresh](../_foundation/authentication/service-tokens.md#token-expiry--refresh)

**Authentication overview updated:**
- Comparison table now shows `token_use` JWT claim values
- Service Token lifetime clarified (5 min signed / 1 hr JWT)
- Note added explaining service vs delegated JWT production
- See: [Authentication Overview](../_foundation/authentication/overview.md)

**Agent callout notes added:**
- `:robot: Agent Note` for JWT refresh pattern and 401 troubleshooting
- `:robot: Agent Note` for delegated vs service token selection
- See: Service Tokens and Overview chapters

### Quick Links for 0.0.3

If you're upgrading from 0.0.2, read these sections:

1. [Delegated Tokens](../_foundation/authentication/service-tokens.md#delegated-tokens-acting-on-behalf-of-users) - When and how to use delegated tokens
2. [Token Expiry & Refresh](../_foundation/authentication/service-tokens.md#token-expiry--refresh) - JWT lifetime and refresh patterns
3. [Quick Reference](../_foundation/authentication/overview.md#quick-reference) - Updated with `token_use` values

---

## [Version 0.2.0](#version-020)

**Released:** 2026-02-14

This release integrates the Guide system into the journey planner documentation.

### What's New

**Guide system documentation:**
- New [Structuring Guide Content](./structuring-guide-content.md) chapter covering the Guide hierarchy, block directive types, audience tags, and authoring best practices
- See: [Structuring Guide Content](./structuring-guide-content.md)

**Concepts expanded:**
- New "Guides & Structured Content" section with foundation glossary includes
- Disambiguation between flow-control directives and guide block directive types
- See: [Concepts - Guides](./concepts.md#guides--structured-content)

**Versioning updated:**
- Guide added to versioned objects table
- Guide-specific versioning notes (no separate -paths/ endpoint)
- See: [Versioning - Guide Versioning](./versioning.md#guide-versioning)

**Directives disambiguation:**
- Added terminology callout distinguishing flow-control directives from guide block directive types
- See: [Directives and JSONLogic](./directives.md)

**Composing Steps & Stages updated:**
- Guide content documentation for stages/steps
- See: [Composing Steps & Stages](./composing-steps-stages.md#guide-content-for-stages-and-steps)

**Agent navigation expanded:**
- Guide content navigation in AGENTS.md
- Guide terminology in quick reference

### Quick Links for 0.2.0

If upgrading from 0.1.0:
1. [Structuring Guide Content](./structuring-guide-content.md) - Start here for the full picture
2. [Concepts - Guides](./concepts.md#guides--structured-content) - Definitions and disambiguation
3. [Versioning - Guide Versioning](./versioning.md#guide-versioning) - Guide-specific versioning

---

## [Version 0.1.0](#version-010)

**Released:** 2026-01-31

This release introduces the Model Taxonomy architecture, fundamentally changing how models are configured for workflows.

### What's New

**Model Taxonomy introduced:**
- New Lineage, Provider, Tier, Release, Variant hierarchy
- Lineage as "prompting language" conceptual model
- See: [Concepts - Model Configuration](./concepts.md#model-configuration)

**InferenceProfiles for model decoupling:**
- Playbooks are now model-agnostic
- InferenceProfiles bind models to prompts at Journey creation
- Enables A/B testing without workflow changes
- See: [Concepts - InferenceProfile](./concepts.md#inference-profile)

**Journey composition clarified:**
- Journey = Playbook + BriefPacks + InferenceProfiles
- Three independent, composable entities
- See: [Concepts - Journey](./concepts.md#journey)

**Glossary expanded:**
- New "Model Taxonomy" section with Lineage, Provider, Tier, Release, Variant
- New "Inference Configuration" section with InferenceProfile terms
- See: [Glossary](./glossary.md)

**Foundation documentation:**
- New [Model Taxonomy](../_foundation/concepts/model-taxonomy.md) concept page
- Shared across Journey Planners and Journey Builder guides

**Terminology changes:**
- `maker_key` → `lineage_key` throughout
- `EngineConfig` → `ModelConfig` in technical references
- `playbook_step_engine_configs` removed (replaced by InferenceProfiles)

### Breaking Changes

This version documents breaking changes to the underlying platform:
- Engine configuration no longer tied to Playbook steps
- Model selection moved to Journey level via InferenceProfiles
- Legacy `maker_key` terminology deprecated

### Quick Links for 0.1.0

If you're upgrading from 0.0.3:
1. [Model Configuration concepts](./concepts.md#model-configuration)
2. [InferenceProfile glossary entry](./glossary.md#inference-configuration)
3. [Foundation Model Taxonomy](../_foundation/concepts/model-taxonomy.md)

---

## Future Versions

### Planned for 0.3.3

- [ ] European Portuguese translations for guide content
- [ ] Additional API cookbook recipes for InferenceProfiles
- [ ] Enhanced mermaid diagrams for model resolution flow

### Planned for 0.4.0

- [ ] Complete bilingual coverage
- [ ] Advanced patterns chapter (complex directive compositions, multi-locale workflows)
- [ ] Performance optimization guide for large guide directories
- [ ] Shared content library best practices and patterns
- [ ] Guide diffing and version comparison tooling
