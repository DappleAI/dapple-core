# Guide Tone and Style Cleanup

**Status:** Draft
**Created:** 2026-01-29
**Last Updated:** 2026-01-29
**Target Version:** N/A (documentation only)
**Dependencies:** None

---

## Executive Summary

This plan addresses tone and style inconsistencies across the documentation guides. The documentation was originally written for internal use at a scrappy startup where competitive energy helped motivate the team. As we prepare to go big, the finger-pointing at competitors comes off as negative and petty.

**Two-phase approach:**
1. **Phase 1: Consolidate duplicates** - Move shared content to `_foundation`, eliminate 3,500+ lines of duplication
2. **Phase 2: Fix tone** - Apply style rules to the single sources

The goal is to shift from "look how bad they are" to "look what we can build together." The documentation should emphasize collaboration: people working together, models working together, technical and non-technical contributors as a sum greater than any individual intelligence. The exasperated energy and punchiness stay. The competitive jabs go.

Additional cleanup: emoji usage, passive voice, second-person pronouns, imperative instructions, em-dashes, and LLM-speak patterns.

---

## Context and Background

### Current State

The documentation is split across two locations:

**`docs/guides/`** (newer, cleaner):
- `api-consumer-guide/` - REST API consumption for frontend developers and integrators
- `journey-developer-guide/` - Backend system internals for platform engineers
- `_foundation/` - Shared concepts, glossary, and authentication documentation

**`crates/dapple-prompt-api/docs/journey-planners-guide/`** (original, needs most work):
- 33 files including intro, concepts, cookbook, walkthrough
- **This is where the competitive/finger-pointing tone is concentrated**
- Also has heavy em-dash usage and emojis

### Problems Identified

1. **Competitive/negative tone** - Finger-pointing at competition, "bitchy" jabs that worked internally but read as petty externally
2. **Em-dashes everywhere** - Dead giveaway for LLM-generated content (`term — definition` pattern)
3. **LLM-speak** - Overly formal, stilted language that sounds like AI wrote it
4. **Emoji audience tags** - Files use `:wrench:`, `:clipboard:`, `:robot:` in audience markers
5. **Passive voice** - "Tasks are executed by the runtime" instead of active constructions
6. **Second person "you"** - "Your UI should present" instead of abstract descriptions
7. **Imperative instructions** - "Use batch operations" instead of declarative statements
8. **Missing inline explanations** - Technical terms introduced without context for newcomers
9. **Inconsistent warmth** - Some sections are welcoming, others are terse

### Relevant Project Patterns

From CLAUDE.md's Journey-Centric Philosophy:
- Internal terms (Brief, BriefPack, BriefGroup) should not be exposed to end users
- Different audiences have different needs (Product, Prompt Engineers, Agents)
- Documentation should support iterative, not linear, workflows

---

## Phase 1: Consolidate Duplicates

Before fixing tone, eliminate duplicate content so we only edit each piece once.

### Duplication Found

| Content | Lines | Status |
|---------|-------|--------|
| `api-patterns.md` | 793 | Identical in api-consumer-guide AND journey-planners-guide |
| `runtime-operations.md` | 889 | Identical in api-consumer-guide AND journey-planners-guide |
| `api-cookbook.md` | 1078 | Identical in api-consumer-guide AND journey-planners-guide |
| `authentication.md` | 747 | journey-planners-guide duplicates _foundation content |
| `glossary.md` | 100 | journey-planners-guide standalone instead of using includes |
| `concepts.md` | 310 | journey-planners-guide standalone with unique author content |
| **Total duplicate lines** | ~3,500+ | |

### Consolidation Tasks

#### Task 1: Move API content to _foundation

Create `docs/guides/_foundation/api/` and move:
- `api-patterns.md` (from api-consumer-guide)
- `runtime-operations.md` (from api-consumer-guide)
- `api-cookbook.md` (from api-consumer-guide)

Then replace both copies with `{{#include}}` references:
- `docs/guides/api-consumer-guide/api-patterns.md` -> includes from _foundation
- `crates/dapple-prompt-api/docs/journey-planners-guide/api-patterns.md` -> includes from _foundation
- Same for runtime-operations.md and api-cookbook.md

#### Task 2: Fix journey-planners-guide authentication

Replace the standalone 747-line `authentication.md` with `{{#include}}` references to `_foundation/authentication/` files, matching how `api-consumer-guide/getting-started/authentication.md` does it.

#### Task 3: Reconcile journey-planners glossary

Replace standalone `glossary.md` with `{{#include}}` reference to `_foundation/glossary.md`.

#### Task 4: Evaluate journey-planners concepts

The `concepts.md` in journey-planners-guide has unique author-focused content:
- Debrief UX Patterns
- Author checklists
- Detailed examples

Options:
1. Move unique content to _foundation with audience tags
2. Keep as local additions that supplement _foundation includes

**Decision needed:** Review the unique content and decide if it belongs in _foundation or stays guide-specific.

### Consolidation Verification

After consolidation:
```bash
# Build all guides to ensure includes work
cd docs/guides/api-consumer-guide && mdbook build
cd docs/guides/journey-developer-guide && mdbook build
cd crates/dapple-prompt-api/docs/journey-planners-guide && mdbook build

# Verify no broken includes
grep -r "{{#include" docs/guides/ | grep -v ".md:" | head -20
```

---

## Phase 2: Style Guide Rules

### Rule 1: Collaboration Over Competition

Replace competitive finger-pointing with collaboration-focused messaging. Keep the energy and exasperation. Lose the pettiness.

**The message we want:**
- Collaboration of people (technical and non-technical working together)
- Collaboration of models (AI and human intelligence as partners)
- The sum is greater than any individual intelligence, artificial or not
- This builds on everything people have done to make this technology possible

**Before:**
```markdown
Unlike most tools that force you into rigid workflows, dapple-prompt adapts to how you actually work.
```

**After:**
```markdown
dapple-prompt adapts to how teams actually work, bringing together technical and non-technical contributors in a shared workflow.
```

**Before:**
```markdown
Traditional LLM wrappers just throw prompts at an API. We built something smarter.
```

**After:**
```markdown
The conduit service coordinates prompts, context, and model selection so teams can focus on what they're building rather than wrestling with infrastructure.
```

**Before:**
```markdown
Most platforms silo AI and human work. That's backwards.
```

**After:**
```markdown
The journey model treats AI and human contributions as parts of the same workflow. Each step builds on the last, regardless of who (or what) did the work.
```

**Pattern to avoid:** "Unlike X...", "Most tools...", "Traditional approaches...", "The problem with..."
**Pattern to use:** Focus on what dapple-prompt enables, how collaboration works, what becomes possible.

---

### Rule 2: No Em-Dashes

Em-dashes (—) are a telltale sign of LLM-generated content. Replace with colons, periods, commas, or restructure the sentence.

**Before:**
```markdown
- **Treat like passwords** — Store in secure credential managers
- **Use environment variables** — Never hardcode in source code
```

**After:**
```markdown
- **Treat like passwords.** Store tokens in secure credential managers.
- **Use environment variables.** Never hardcode tokens in source code.
```

**Before:**
```markdown
The token itself contains no readable information—it's just a random string.
```

**After:**
```markdown
The token itself contains no readable information. It's just a random string.
```

**Before:**
```markdown
- `anthropic` — Anthropic Claude (default)
- `mistral` — Local Mistral via Candle
```

**After:**
```markdown
- `anthropic`: Anthropic Claude (default)
- `mistral`: Local Mistral via Candle
```

---

### Rule 3: Write Like a Human

Avoid LLM-speak. The documentation should sound like a person wrote it, not a language model. Use proper grammar and punctuation, but keep sentences natural.

**LLM-speak patterns to avoid:**
- Overly formal constructions ("It is important to note that...")
- Excessive hedging ("It should be noted that this may potentially...")
- Stiff transitions ("Furthermore...", "Additionally...", "Moreover...")
- Redundant phrases ("In order to...", "Due to the fact that...")
- Overuse of "utilize" instead of "use"
- Lists that all start with the same word/structure

**Before:**
```markdown
It is important to note that the API utilizes batch semantics for all create operations. Furthermore, this design enables efficient bulk operations.
```

**After:**
```markdown
The API uses batch semantics for all create operations. This makes bulk operations efficient.
```

**Before:**
```markdown
Additionally, it should be noted that tokens are validated prior to request processing.
```

**After:**
```markdown
The server validates tokens before processing requests.
```

---

### Rule 4: No Emojis

Remove all emojis from documentation, especially audience tags.

**Before:**
```markdown
> **Audience:** :wrench: Prompt Engineers · :clipboard: Product · :robot: Agents
```

**After:**
```markdown
> **Audience:** Prompt Engineers, Product, Agents
```

**Rationale:** Emojis reduce professionalism, may not render correctly in all contexts, and add visual noise without adding meaning.

---

### Rule 2: Active Voice

Always use active voice. The subject performs the action.

**Before:**
```markdown
Tasks are executed by the runtime when the dispatch begins.
```

**After:**
```markdown
The runtime executes tasks when the dispatch begins.
```

**Before:**
```markdown
The token is validated by the API server before processing the request.
```

**After:**
```markdown
The API server validates the token before processing the request.
```

**Before:**
```markdown
Errors are returned in a standard JSON format.
```

**After:**
```markdown
The API returns errors in a standard JSON format.
```

**Rationale:** Active voice is clearer, more direct, and easier to read. It makes the actor explicit, which is especially important in technical documentation.

---

### Rule 3: Third Person / Abstract Descriptions

Use third person or abstract descriptions instead of second person "you".

**Before:**
```markdown
Your UI should present all BriefUnits from a single BriefGroup as one form/step.
```

**After:**
```markdown
The UI presents all BriefUnits from a single BriefGroup as one form or step.
```

**Before:**
```markdown
You can access input via the environment variable.
```

**After:**
```markdown
Scripts access input via the environment variable.
```

**Before:**
```markdown
You need to fetch the briefs before rendering the debrief UI.
```

**After:**
```markdown
Applications fetch the briefs before rendering the debrief UI.
```

**Before:**
```markdown
Before diving into API calls, understand how users experience the system.
```

**After:**
```markdown
Before examining API calls, this section explains how users experience the system.
```

**Rationale:** Third person maintains professional distance and works better for diverse audiences (humans and AI agents). It also avoids assumptions about who is reading.

---

### Rule 4: Warm, Welcoming Tone

Maintain a warm, welcoming tone throughout all pages. Add brief orienting sentences at the start of sections to help readers understand context.

**Before:**
```markdown
## Error Response Format

### Standard Error Response

```json
{
  "status": "fail",
  "message": "Detailed error description"
}
```
```

**After:**
```markdown
## Error Response Format

This section describes the error response structures that the API returns. Understanding these formats helps applications provide meaningful feedback to users.

### Standard Error Response

The API returns errors in a consistent JSON structure:

```json
{
  "status": "fail",
  "message": "Detailed error description"
}
```
```

**Rationale:** Orienting sentences help readers understand why content matters and how it connects to their goals. This creates a more welcoming experience.

---

### Rule 5: Declarative Style for Instructions

Use declarative style instead of imperative commands.

**Before:**
```markdown
Use batch operations instead of individual requests.
```

**After:**
```markdown
Batch operations are preferred over individual requests.
```

**Before:**
```markdown
Check request schema against OpenAPI spec.
```

**After:**
```markdown
The request schema should match the OpenAPI specification.
```

**Before:**
```markdown
Verify field types match expected types.
```

**After:**
```markdown
Field types must match expected types.
```

**Before:**
```markdown
1. Check request schema against OpenAPI spec
2. Ensure all required fields are present
3. Verify field types match expected types
```

**After:**
```markdown
Resolution steps:
1. The request schema should match the OpenAPI specification
2. All required fields must be present
3. Field types must match expected types
```

**Rationale:** Declarative style describes what is correct rather than commanding the reader. This is less prescriptive and works better for reference documentation.

---

### Rule 6: Inline Explanations with Audience Tags

Always explain technical terms inline with audience tags so informed readers can skip.

**Pattern:**
```markdown
> **For newcomers:** A JWT (JSON Web Token) is a self-contained token that encodes claims (user ID, roles, expiration) and can be validated without database lookup.
```

**Example in context:**
```markdown
The API uses JWTs for web session authentication.

> **For newcomers:** A JWT (JSON Web Token) is a self-contained token that encodes claims such as user ID, roles, and expiration time. Unlike opaque tokens, JWTs can be validated without a database lookup because all necessary information is encoded in the token itself.
```

**Terms requiring inline explanations:**
- JWT (JSON Web Token)
- PAT (Personal Access Token)
- CTE (Common Table Expression)
- UUID (Universally Unique Identifier)
- UNNEST (PostgreSQL array expansion)
- Idempotent/Idempotency
- Atomic/Atomicity
- Versioned Object
- Principal Object
- Owned Dependent
- Shared Dependent

**Rationale:** Inline explanations make documentation accessible to newcomers without slowing down experts who can skip the explanation blocks.

---

## Files to Audit

### Priority 1: High-Traffic Files (API Consumer Guide)

These files are most commonly accessed by external developers and integrations.

| File | Issues Identified | Status |
|------|-------------------|--------|
| `api-consumer-guide/api-patterns.md` | Emoji audience tags (`:wrench:`, `:clipboard:`, `:robot:`), some passive voice | [ ] Pending |
| `api-consumer-guide/runtime-operations.md` | Emoji audience tags, second person ("You'll learn"), imperative style in best practices | [ ] Pending |
| `api-consumer-guide/error-handling.md` | Imperative style ("Check", "Verify", "Use"), passive voice | [ ] Pending |
| `api-consumer-guide/getting-started/authentication.md` | Mixed voice, some second person | [ ] Pending |
| `api-consumer-guide/api-cookbook.md` | Review needed | [ ] Pending |
| `api-consumer-guide/preface.md` | Second person throughout | [ ] Pending |
| `api-consumer-guide/endpoint-reference.md` | Review needed | [ ] Pending |
| `api-consumer-guide/getting-started/intro.md` | Review needed | [ ] Pending |
| `api-consumer-guide/getting-started/concepts.md` | Review needed | [ ] Pending |
| `api-consumer-guide/getting-started/glossary.md` | Review needed | [ ] Pending |

### Priority 2: Foundation Files (Shared Content)

These files are included in multiple guides, so fixes propagate widely.

| File | Issues Identified | Status |
|------|-------------------|--------|
| `_foundation/concepts/briefs-and-packs.md` | Second person ("Your UI should present") | [ ] Pending |
| `_foundation/glossary.md` | Review needed | [ ] Pending |
| `_foundation/authentication/overview.md` | Emoji (`:robot:`), mostly good otherwise | [ ] Pending |
| `_foundation/authentication/api-tokens.md` | Review needed | [ ] Pending |
| `_foundation/authentication/web-sessions.md` | Review needed | [ ] Pending |
| `_foundation/authentication/service-tokens.md` | Review needed | [ ] Pending |
| `_foundation/authentication/ethereum-tokens.md` | Review needed | [ ] Pending |
| `_foundation/authentication/best-practices.md` | Likely imperative style | [ ] Pending |
| `_foundation/concepts/versioning-model.md` | Review needed | [ ] Pending |
| `_foundation/concepts/core-execution-model.md` | Review needed | [ ] Pending |
| `_foundation/concepts/environment-variables.md` | Review needed | [ ] Pending |
| `_foundation/concepts/artifacts-and-files.md` | Review needed | [ ] Pending |

### Priority 3: Journey Developer Guide

These files target backend developers with more technical content.

| File | Issues Identified | Status |
|------|-------------------|--------|
| `journey-developer-guide/flow/nushell-execution.md` | Mixed voice, some second person, passive voice | [ ] Pending |
| `journey-developer-guide/preface.md` | Second person throughout | [ ] Pending |
| `journey-developer-guide/data-layer/query-patterns.md` | Imperative ("Don't do this", "Do this instead") | [ ] Pending |
| `journey-developer-guide/data-layer/model-patterns.md` | Review needed | [ ] Pending |
| `journey-developer-guide/data-layer/migration-guide.md` | Review needed | [ ] Pending |
| `journey-developer-guide/flow/overview.md` | Review needed | [ ] Pending |
| `journey-developer-guide/flow/step-engine.md` | Review needed | [ ] Pending |
| `journey-developer-guide/flow/dataflow-integration.md` | Review needed | [ ] Pending |
| `journey-developer-guide/flow/security-model.md` | Review needed | [ ] Pending |
| `journey-developer-guide/conduit/overview.md` | Review needed | [ ] Pending |
| `journey-developer-guide/conduit/llm-backends.md` | Review needed | [ ] Pending |
| `journey-developer-guide/conduit/embedding-backends.md` | Review needed | [ ] Pending |
| `journey-developer-guide/conduit/template-rendering.md` | Review needed | [ ] Pending |
| `journey-developer-guide/conduit/overlay-system.md` | Review needed | [ ] Pending |
| `journey-developer-guide/nushell/custom-commands.md` | Review needed | [ ] Pending |
| `journey-developer-guide/nushell/command-authoring.md` | Review needed | [ ] Pending |
| `journey-developer-guide/nushell/task-template.md` | Review needed | [ ] Pending |
| `journey-developer-guide/development/testing-patterns.md` | Review needed | [ ] Pending |
| `journey-developer-guide/development/local-development.md` | Review needed | [ ] Pending |
| `journey-developer-guide/development/debugging.md` | Review needed | [ ] Pending |
| `journey-developer-guide/getting-started/intro.md` | Review needed | [ ] Pending |
| `journey-developer-guide/getting-started/glossary.md` | Review needed | [ ] Pending |
| `journey-developer-guide/getting-started/concepts.md` | Review needed | [ ] Pending |
| `journey-developer-guide/getting-started/architecture-overview.md` | Review needed | [ ] Pending |

### Priority 4: Metadata Files (docs/guides/)

| File | Issues Identified | Status |
|------|-------------------|--------|
| `api-consumer-guide/AGENTS.md` | Review needed | [ ] Pending |
| `api-consumer-guide/SUMMARY.md` | Review needed | [ ] Pending |
| `journey-developer-guide/AGENTS.md` | Review needed | [ ] Pending |
| `journey-developer-guide/SUMMARY.md` | Review needed | [ ] Pending |

### Priority 0: Journey Planners Guide (HIGHEST PRIORITY)

**Location:** `crates/dapple-prompt-api/docs/journey-planners-guide/`

This is the original guide and has the most tone issues. Start here.

| File | Issues Identified | Status |
|------|-------------------|--------|
| `intro.md` | **CRITICAL:** Competitive finger-pointing, broken grammar, em-dashes, emojis | [ ] Pending |
| `preface.md` | Review for competitive tone | [ ] Pending |
| `concepts.md` | Em-dashes, review for tone | [ ] Pending |
| `glossary.md` | Em-dashes in definitions | [ ] Pending |
| `cookbook.md` | Review needed | [ ] Pending |
| `api-cookbook.md` | Review needed | [ ] Pending |
| `api-patterns.md` | Em-dashes, review for tone | [ ] Pending |
| `authentication.md` | Em-dashes | [ ] Pending |
| `runtime-operations.md` | Review needed | [ ] Pending |
| `resume-walkthrough.md` | Review needed | [ ] Pending |
| `dapple-prompt-flow.md` | Review needed | [ ] Pending |
| `dapple-prompt-conduit.md` | Review needed | [ ] Pending |
| `composing-steps-stages.md` | Review needed | [ ] Pending |
| `adding-prompts.md` | Review needed | [ ] Pending |
| `prompt-authoring.md` | Review needed | [ ] Pending |
| `brief-pack-manifest.md` | Review needed | [ ] Pending |
| `directives.md` | Review needed | [ ] Pending |
| `fluent-messages.md` | Review needed | [ ] Pending |
| `nushell-task-template.md` | Review needed | [ ] Pending |
| `custom-nu-commands.md` | Review needed | [ ] Pending |
| `testing-debugging.md` | Review needed | [ ] Pending |
| `versioning.md` | Review needed | [ ] Pending |
| `templates.md` | Review needed | [ ] Pending |
| `appendix.md` | Review needed | [ ] Pending |
| `authors-notes.md` | Review needed | [ ] Pending |
| `AGENTS.md` | Emojis | [ ] Pending |
| `SUMMARY.md` | Review needed | [ ] Pending |
| `CHANGES.md` | Review needed | [ ] Pending |
| `examples/*.md` (4 files) | Review needed | [ ] Pending |

---

## Specific Edits Required

### journey-planners-guide/intro.md (CRITICAL)

This file has the worst tone issues and needs a full rewrite of the opening.

**Lines 1-11: Replace competitive opening with collaboration focus**

```markdown
# Before
Most conversations around AI treat artificial intelligence as though the limits of intelligence by itself aren't readily clear throughout all of human history. The ability and likely level of interest while applying to something making use of artificial intelligence just like it would the natural variety.



**But here's what those companies miss: intelligence alone never built anything worthwhile. Collaboration did.**

Dapple's Playbooks harness the strengths that come from the ebb and flow of multiple intelligent perspectives beings' paths' collide.

varied intelligences—among LLMs, yes, but more importantly among people—to build plans that use the right tool for each individual task.

# After
Intelligence alone never built anything worthwhile. Collaboration did.

Dapple's Playbooks bring together different kinds of intelligence: LLMs for pattern recognition and language, scripts for deterministic logic, and people for judgment and creativity. Each contributes what it does best. The result is greater than any individual part.

This isn't about replacing human work with AI. It's about creating workflows where technical and non-technical contributors, models and people, all work in concert. The Playbook coordinates. The Journey guides users through. And useful things get done.
```

**Lines 22-28: Remove competitive jab, keep the energy**

```markdown
# Before
- **The right tool for the job** — Sometimes you need Claude to extract meaning from messy text. Sometimes you need a three-line script to format a date. Playbooks don't make you choose between them—they let you use both exactly when and where they make sense.

# After
- **The right tool for the job.** Sometimes Claude extracts meaning from messy text. Sometimes a three-line script formats a date. Playbooks coordinate both, using each where it makes sense.
```

```markdown
# Before
Most of the Journey is deterministic code doing deterministic things. The LLM gets called at specific points where you actually need intelligence and flexibility. This isn't "sprinkle AI everywhere and hope"—it's deliberate collaboration between different kinds of intelligence, all working in concert.

# After
Most of the Journey is deterministic code doing deterministic things. The LLM gets called at specific points where flexibility and pattern recognition matter. Deliberate collaboration between different kinds of intelligence, all working in concert.
```

**Lines 50-59: Remove emojis from audience table**

```markdown
# Before
| Icon | Audience | Role |
|------|----------|------|
| 🔧 | **Journey Developers** | Technical builders writing Nushell scripts, LLM prompts, and implementation logic |
| 🎨 | **Journey Designers** | Domain experts who understand the real-world process being automated |
| 📋 | **Product** | Strategic readers building vocabulary and mental models to engage meaningfully with the system |
| 🤖 | **Agents** | LLMs learning to consume the API, build integrations, and work within the framework |

# After
| Audience | Role |
|----------|------|
| **Journey Developers** | Technical builders writing Nushell scripts, LLM prompts, and implementation logic |
| **Journey Designers** | Domain experts who understand the real-world process being automated |
| **Product** | Strategic readers building vocabulary and mental models |
| **Agents** | LLMs learning to consume the API and build integrations |
```

```markdown
# Before
> **🤖 Agents:** Start with [AGENTS.md](./AGENTS.md) for a navigation guide tailored to your task.

# After
> **For agents:** Start with [AGENTS.md](./AGENTS.md) for a navigation guide tailored to the task at hand.
```

---

### api-consumer-guide/api-patterns.md

**Line 3 - Remove emojis from audience tag:**
```markdown
# Before
> **Audience:** :wrench: Prompt Engineers · :clipboard: Product · :robot: Agents

# After
> **Audience:** Prompt Engineers, Product, Agents
```

**Lines 32-36 - Remove emoji and adjust agent note:**
```markdown
# Before
> **:robot: API Consumer Agent Note:** For programmatic access, use API tokens (`dpl_` prefix) instead
> of JWTs. API tokens are simpler to manage and don't require OAuth flows. See
> [Authentication](./authentication.md) for token creation. Remember:
> - API tokens have rate limit of 60 req/min (vs 300 for web sessions)
> - Detect token type by prefix: `dpl_` → API token, no prefix → JWT

# After
> **For API consumer agents:** For programmatic access, API tokens (`dpl_` prefix) are simpler to manage than JWTs and do not require OAuth flows. See [Authentication](./authentication.md) for token creation.
> - API tokens have a rate limit of 60 req/min (versus 300 for web sessions)
> - Token type detection: `dpl_` prefix indicates API token, no prefix indicates JWT
```

### api-consumer-guide/runtime-operations.md

**Line 3 - Remove emojis from audience tag:**
```markdown
# Before
> **Audience:** :wrench: Prompt Engineers · :clipboard: Product · :robot: Agents

# After
> **Audience:** Prompt Engineers, Product, Agents
```

**Line 5 - Remove second person:**
```markdown
# Before
This chapter covers the journey-centric runtime operations for consuming the dapple-prompt API in production. You'll learn how to start journeys, fetch debrief content, collect user input, execute workflows, and handle iterative flows.

# After
This chapter covers the journey-centric runtime operations for consuming the dapple-prompt API in production. Topics include starting journeys, fetching debrief content, collecting user input, executing workflows, and handling iterative flows.
```

**Lines 59-62 - Remove emoji and adjust agent note:**
```markdown
# Before
> **:robot: Journey Orchestration Agent Note:** When creating journeys programmatically:
> - Use API tokens (`dpl_` prefix) for authentication - see [Authentication](./authentication.md)
> - The journey `object_id` is your primary reference for all subsequent operations
> - Store both `journey_object_id` and the assigned `brief_pack_object_ids` for the session

# After
> **For journey orchestration agents:** When creating journeys programmatically:
> - API tokens (`dpl_` prefix) are recommended for authentication - see [Authentication](./authentication.md)
> - The journey `object_id` serves as the primary reference for all subsequent operations
> - Both `journey_object_id` and the assigned `brief_pack_object_ids` should be stored for the session
```

### api-consumer-guide/error-handling.md

**Lines 47-50 - Convert imperative to declarative:**
```markdown
# Before
**Resolution:**
1. Check request schema against OpenAPI spec
2. Ensure all required fields are present
3. Verify field types match expected types
4. Check string length and format constraints

# After
**Resolution:**
1. The request schema should match the OpenAPI specification
2. All required fields must be present
3. Field types must match expected types
4. String length and format constraints must be satisfied
```

**Lines 69-72 - Convert imperative to declarative:**
```markdown
# Before
**Resolution:**
1. Include `Authorization: Bearer <token>` header
2. Check token expiration and refresh if needed
3. Verify token format (JWT or `dpl_` prefix)
4. Generate new token if revoked

# After
**Resolution:**
1. The `Authorization: Bearer <token>` header must be included
2. Token expiration should be checked and tokens refreshed as needed
3. Token format must be correct (JWT or `dpl_` prefix)
4. A new token is required if the previous one was revoked
```

### _foundation/concepts/briefs-and-packs.md

**Lines 78-79 - Remove second person:**
```markdown
# Before
**Design implications:**
- Your UI should present all BriefUnits from a single BriefGroup as one form/step

# After
**Design implications:**
- The UI presents all BriefUnits from a single BriefGroup as one form or step
```

### _foundation/authentication/overview.md

**Line 78 - Remove emoji:**
```markdown
# Before
> :robot: **Agent Note:** When using Service Tokens, remember that the signed token is short-lived (max 5 min), but the resulting JWT lasts 1 hour. If you receive unexpected 401 errors, the JWT has likely expired—request a new one via `POST /api/get-service-token`.

# After
> **For agents:** When using Service Tokens, the signed token is short-lived (max 5 min), but the resulting JWT lasts 1 hour. Unexpected 401 errors may indicate JWT expiration; a new token can be requested via `POST /api/get-service-token`.
```

### journey-developer-guide/flow/nushell-execution.md

**Lines 79-81 - Remove second person:**
```markdown
# Before
### Accessing Environment

```nushell
# Access input via environment
let input = $in

# After
### Accessing Environment

```nushell
# Scripts access input via environment
let input = $in
```

### journey-developer-guide/data-layer/query-patterns.md

**Lines 138-160 - Convert imperative anti-pattern to declarative:**
```markdown
# Before
## Anti-Pattern: Separate ID Lookups

**Don't do this:**
```rust
// Multiple round-trips
let brief_ids = get_brief_internal_ids(&pool, &brief_object_ids).await?;
...
```

**Do this instead:**
```sql
-- Single query with CTE
...
```

# After
## Anti-Pattern: Separate ID Lookups

This anti-pattern demonstrates what to avoid.

**Problematic approach (multiple round-trips):**
```rust
// This causes multiple database round-trips
let brief_ids = get_brief_internal_ids(&pool, &brief_object_ids).await?;
...
```

**Preferred approach (single round-trip with CTE):**
```sql
-- Single query with CTE
...
```
```

---

## Implementation Guidance

### For Implementation Agents

1. **Start with Priority 0 (journey-planners-guide)** - This has the most tone issues
2. **intro.md first** - The competitive finger-pointing is concentrated here
3. **Apply all nine rules to each file:**
   - Remove competitive/finger-pointing language (Rule 1)
   - Remove em-dashes (Rule 2)
   - Remove LLM-speak (Rule 3)
   - Remove emojis (Rule 4)
   - Use active voice (Rule 5)
   - Use third person (Rule 6)
   - Maintain warmth (Rule 7)
   - Use declarative style (Rule 8)
   - Add inline explanations (Rule 9)
4. **Preserve technical accuracy** - Style changes must not alter meaning
5. **Maintain anchor tags** - Many files use `<!-- ANCHOR: ... -->` for includes; do not remove these
6. **Test after each file** - Ensure markdown renders correctly

### Search Patterns for Issues

Use these patterns to find issues:

**Competitive/finger-pointing language:**
```
those companies
companies miss
unlike most
most tools
most platforms
traditional
the problem with
sprinkle AI
```

**Em-dashes (the big one):**
```
—
```
Note: Em-dashes are used heavily in definition lists (`**Term** — Definition`). Replace with colons or periods.

**Emojis:**
```
🔧 🎨 📋 🤖
:wrench:
:clipboard:
:robot:
:warning:
:information_source:
```

**Second person:**
```
you'll
you can
you need
you should
your
You
```

**Passive voice indicators:**
```
is executed
are created
was validated
are returned
is stored
is required
```

**Imperative verbs at line start:**
```
^Check
^Verify
^Use
^Ensure
^Create
^Run
^Add
^Remove
^Set
^Include
```

**LLM-speak patterns:**
```
It is important to note
Furthermore
Additionally
Moreover
In order to
Due to the fact that
utilize (instead of use)
```

---

## Verification

### Checklist for Each File

For each file being updated, verify:

- [ ] No competitive/finger-pointing language
- [ ] No em-dashes (use colons, periods, or restructure)
- [ ] No LLM-speak patterns
- [ ] No emojis remain
- [ ] All audience tags use plain text format
- [ ] Active voice throughout
- [ ] No second person pronouns ("you", "your")
- [ ] Declarative style (not imperative)
- [ ] Technical terms have inline explanations where needed
- [ ] Section introductions provide context (warmth)
- [ ] Markdown renders correctly
- [ ] Anchor tags preserved
- [ ] Links still work

### Build Verification

After completing edits:

```bash
# Build all guides
cd docs/guides/api-consumer-guide && mdbook build
cd docs/guides/journey-developer-guide && mdbook build

# Test for broken links
mdbook test docs/guides/api-consumer-guide
mdbook test docs/guides/journey-developer-guide
```

---

## Progress Tracking

### Summary

| Priority | Location | Total Files | Completed | Remaining |
|----------|----------|-------------|-----------|-----------|
| Priority 0 (Journey Planners) | `crates/.../journey-planners-guide/` | 33 | 0 | 33 |
| Priority 1 (API Consumer) | `docs/guides/api-consumer-guide/` | 10 | 0 | 10 |
| Priority 2 (Foundation) | `docs/guides/_foundation/` | 12 | 0 | 12 |
| Priority 3 (Journey Developer) | `docs/guides/journey-developer-guide/` | 24 | 0 | 24 |
| Priority 4 (Metadata) | `docs/guides/*/` | 4 | 0 | 4 |
| **Total** | | **83** | **0** | **83** |

---

## Open Questions and Risks

### Open Questions

1. **Code comments**: Code comments inside code blocks should follow the same rules (third person, active voice).

2. **Agent notes format**: Use `> **For agents:**` or `> **For newcomers:**` prefix style (no emojis).

### Risks

1. **Meaning changes** - Style edits could inadvertently change technical meaning
   - Mitigation: Technical review after each batch of edits

2. **Inconsistent application** - Different editors may apply rules differently
   - Mitigation: This plan provides specific before/after examples

3. **Readability regression** - Some passive constructions may be clearer in context
   - Mitigation: Allow justified exceptions with comments

---

## Implementation Notes

### Batch Processing Strategy

Process files in this order:

**Phase 1: Kill the competitive tone (Priority 0)**
1. `journey-planners-guide/intro.md` (CRITICAL: competitive finger-pointing, broken grammar)
2. `journey-planners-guide/preface.md` (review for competitive tone)
3. `journey-planners-guide/concepts.md` (em-dashes, tone review)
4. Remaining journey-planners-guide files

**Phase 2: Clean up docs/guides/ (Priorities 1-4)**
5. `api-consumer-guide/api-patterns.md` (emojis, em-dashes)
6. `api-consumer-guide/runtime-operations.md` (emojis, second person)
7. `api-consumer-guide/error-handling.md` (imperative style)
8. `_foundation/` files (em-dashes throughout)
9. Remaining Priority 1-4 files

### Review Process

1. Complete one priority tier
2. Run build verification
3. Review for consistency
4. Proceed to next tier
