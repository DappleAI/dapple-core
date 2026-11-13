# Documentation Architecture Refactor

**Status:** Draft
**Created:** 2025-01-28
**Last Updated:** 2025-01-28
**Target Version:** 0.1.0

---

## Executive Summary

This plan reorganizes the dapple-prompt documentation from a monolithic mdBook into a multi-guide, multilingual architecture with shared foundational content. The current "Journey Planners Guide" combines three distinct audiences (playbook authors, backend developers, and API consumers) into a single book, creating confusion and making it difficult to extend with new guides.

The proposed structure creates three dedicated guides:
1. **API Consumer Guide** — For frontend developers and integration partners consuming the REST API
2. **Journey Developer Guide** — For backend developers working on system internals (flow engine, conduit, nushell commands)
3. **Journey Planner Guide** — For playbook authors designing workflows, briefs, and user experiences

Additionally, the architecture includes:
- A shared foundation layer for common content (glossary, core concepts, authentication)
- Multilingual support using Fluent for localization
- A scalable structure for future guides

The refactor uses mdBook's `{{#include}}` directive for content sharing and Fluent's `.ftl` files for translatable strings.

---

## Context and Background

### Current State

The documentation currently exists as a single mdBook at:
```
crates/dapple-prompt-api/docs/journey-planners-guide/
```

**Current Structure:**
```
journey-planners-guide/
  book.toml
  SUMMARY.md
  preface.md
  AGENTS.md
  intro.md

  # Foundation (should be shared)
  glossary.md           # Terms used by all audiences
  concepts.md           # Core concepts, data model

  # Authoring (Journey Planner specific)
  nushell-task-template.md
  composing-steps-stages.md
  versioning.md
  resume-walkthrough.md
  brief-pack-manifest.md
  testing-debugging.md
  directives.md

  # Runtime (Journey Planner specific)
  dapple-prompt-flow.md
  custom-nu-commands.md

  # Prompts & LLM (Journey Planner specific)
  prompt-authoring.md
  dapple-prompt-conduit.md
  adding-prompts.md
  fluent-messages.md

  # API Consumer (should be separate guide)
  authentication.md
  api-patterns.md
  runtime-operations.md
  api-cookbook.md

  # Reference (mixed)
  cookbook.md
  authors-notes.md
  CHANGES.md
  appendix.md
  templates.md

  # Examples
  examples/
    simple-nu-task.md
    ordered-setup-step.md
    resume-playbook.md
    prompt-templates.md
```

### Problems with Current Structure

1. **Mixed audiences** - The guide tries to serve playbook authors and API consumers simultaneously, leading to unclear navigation
2. **Embedded API content** - API consumer documentation is buried in an "authoring" guide
3. **No scalability** - Adding new guides (e.g., "Administrator Guide", "Operator Guide") requires duplicating foundation content
4. **Awkward location** - Documentation lives inside `dapple-prompt-api` crate but serves the entire platform

### Relevant Project Patterns

From CLAUDE.md's Journey-Centric Philosophy:
- Internal terms (Brief, BriefPack, BriefGroup) should not be exposed to end users
- Different audiences have different needs (Product, Prompt Engineers, Agents)
- Documentation should support iterative, not linear, workflows

---

## Proposed Directory Structure

Move documentation to a top-level `docs/guides/` directory with the following structure:

```
docs/
  guides/
    # Shared foundation (not a standalone book)
    _foundation/
      glossary.md                    # Shared glossary
      concepts/
        core-execution-model.md      # Task, Step, Stage, Playbook, Journey
        briefs-and-packs.md          # Brief, BriefGroup, BriefPack
        artifacts-and-files.md       # artifact.json, checkpoint.json, etc.
        environment-variables.md     # Runtime env vars
        versioning-model.md          # How versioning works
      authentication/
        overview.md                  # Auth overview and comparison table
        web-sessions.md              # NextAuth JWT
        api-tokens.md                # Opaque PAT (dpl_)
        service-tokens.md            # Ed25519 service auth
        ethereum-tokens.md           # Ethereum-signed external auth
        best-practices.md            # Security checklist
      data-model/
        principal-objects.md         # Task, Playbook, Journey, etc.
        versioned-objects.md         # ValueSlot, Brief, Asset, etc.
        associative-tables.md        # How many-to-many works

    # Fluent localization files (shared across guides)
    _locales/
      en/                            # English (source)
        common.ftl                   # Shared UI strings
        glossary.ftl                 # Glossary term translations
        errors.ftl                   # Error messages
      pt-BR/                         # Brazilian Portuguese
        common.ftl
        glossary.ftl
        errors.ftl
      pt-PT/                         # European Portuguese
        common.ftl
        glossary.ftl
        errors.ftl

    # API Consumer Guide (new)
    api-consumer-guide/
      book.toml
      SUMMARY.md
      preface.md
      AGENTS.md

      # Localized content directories
      en/                            # English (source language)
        # Content mirrors structure below
      pt-BR/                         # Translations
      pt-PT/

      # Getting Started (includes from foundation)
      getting-started/
        intro.md                     # API Consumer specific intro
        glossary.md                  # {{#include ../_foundation/glossary.md}}
        concepts.md                  # Curated includes + API-specific content
        authentication.md            # {{#include ../_foundation/authentication/...}}

      # Core Guide Content
      api-patterns.md                # Batch semantics, response formats
      runtime-operations.md          # Journey-centric API workflows
      api-cookbook.md                # Copy-paste recipes

      # Reference
      endpoint-reference.md          # Full endpoint documentation
      error-handling.md              # Error codes and troubleshooting

    # Journey Developer Guide (new - backend system internals)
    journey-developer-guide/
      book.toml
      SUMMARY.md
      preface.md
      AGENTS.md

      # Localized content directories
      en/
      pt-BR/
      pt-PT/

      # Getting Started
      getting-started/
        intro.md                     # Developer-focused intro
        glossary.md                  # {{#include ../_foundation/glossary.md}}
        concepts.md                  # System architecture concepts
        architecture-overview.md     # System component diagram

      # Flow Engine (dapple-prompt-flow)
      flow/
        overview.md                  # Flow engine architecture
        nushell-execution.md         # Nushell execution model
        step-engine.md               # StepEngine internals
        dataflow-integration.md      # dataflow-rs integration
        security-model.md            # Nushell sandboxing, allowed commands

      # Conduit Service (dapple-prompt-conduit)
      conduit/
        overview.md                  # Conduit architecture
        llm-backends.md              # Anthropic, Mistral, Stub backends
        embedding-backends.md        # ModernBERT, embedding services
        template-rendering.md        # Handlebars template system
        overlay-system.md            # Slot visibility, semantic ranking

      # Nushell Commands
      nushell/
        custom-commands.md           # Custom Nu command reference
        command-authoring.md         # Writing new Nu commands
        plugin-system.md             # Plugin architecture

      # Data Layer (dapple-data)
      data-layer/
        query-patterns.md            # SQL query conventions
        model-patterns.md            # Rust model conventions
        migration-guide.md           # Writing migrations

      # Testing & Development
      development/
        testing-patterns.md          # Integration test patterns
        local-development.md         # Dev environment setup
        debugging.md                 # Debugging techniques

    # Journey Planner Guide (refactored - playbook authoring)
    journey-planner-guide/
      book.toml
      SUMMARY.md
      preface.md
      AGENTS.md

      # Localized content directories
      en/
      pt-BR/
      pt-PT/

      # Getting Started (includes from foundation)
      getting-started/
        intro.md                     # Journey Planner specific intro
        glossary.md                  # {{#include ../_foundation/glossary.md}}
        concepts.md                  # Curated includes + authoring-specific content
        directives.md                # JSONLogic, conditionals

      # Authoring Playbooks
      authoring/
        composing-steps-stages.md    # Building workflows
        versioning.md                # Version management
        testing-debugging.md         # Testing playbooks

      # Brief System
      briefs/
        brief-pack-manifest.md       # Pack manifest format
        brief-units.md               # UI elements (input, output, markdown)
        debrief-patterns.md          # UX patterns for debrief

      # Prompts & LLM
      prompts/
        prompt-authoring.md          # Writing effective prompts
        adding-prompts.md            # Adding prompts to packs
        fluent-messages.md           # Localized messages

      # Walkthroughs
      walkthroughs/
        resume-walkthrough.md        # End-to-end resume builder

      # Reference
      cookbook.md
      authors-notes.md
      templates.md
      appendix.md

      # Examples
      examples/
        simple-nu-task.md
        ordered-setup-step.md
        resume-playbook.md
        prompt-templates.md

  # Existing directories (unchanged)
  plans/
  changes/
  musings/
```

---

## Multilingual Architecture

### Fluent Localization System

The documentation uses [Project Fluent](https://projectfluent.org/) for localization, the same system used internally by dapple-prompt for user-facing messages.

**Why Fluent:**
- Already used in the codebase (`fluent-messages` system)
- Supports complex pluralization, gender, and grammatical rules
- Clean separation of translatable strings from content
- Can be validated at build time

### Language Support

| Language | Code | Status |
|----------|------|--------|
| English | `en` | Source (always complete) |
| Brazilian Portuguese | `pt-BR` | Primary translation target |
| European Portuguese | `pt-PT` | Secondary translation target |

### Localization File Structure

```
_locales/
  en/
    common.ftl          # Shared UI strings (buttons, labels)
    glossary.ftl        # Glossary term definitions
    errors.ftl          # Error messages
    guide-titles.ftl    # Guide and chapter titles
  pt-BR/
    common.ftl
    glossary.ftl
    errors.ftl
    guide-titles.ftl
  pt-PT/
    common.ftl
    glossary.ftl
    errors.ftl
    guide-titles.ftl
```

### Fluent Message Examples

**`_locales/en/glossary.ftl`:**
```ftl
# Core execution model terms
term-task = Task
term-task-def = A single unit of work implemented as a small script.
    Tasks have clear inputs, outputs, and exit behavior.

term-step = Setup Step
term-step-def = An ordered collection of Tasks that run together
    to accomplish a subgoal.

term-journey = Journey
term-journey-def = A product-level story or experience composed of
    one or more Playbooks, UX decisions, branching rules, and
    longer-lived state.

# Audience tags
audience-prompt-engineer = Prompt Engineers
audience-product = Product
audience-agent = Agents
audience-developer = Developers
```

**`_locales/pt-BR/glossary.ftl`:**
```ftl
term-task = Tarefa
term-task-def = Uma única unidade de trabalho implementada como
    um pequeno script. Tarefas têm entradas, saídas e comportamento
    de saída claros.

term-step = Etapa de Configuração
term-step-def = Uma coleção ordenada de Tarefas que executam juntas
    para alcançar um subobjetivo.

term-journey = Jornada
term-journey-def = Uma história ou experiência em nível de produto
    composta por um ou mais Playbooks, decisões de UX, regras de
    ramificação e estado de longa duração.

audience-prompt-engineer = Engenheiros de Prompt
audience-product = Produto
audience-agent = Agentes
audience-developer = Desenvolvedores
```

### mdBook i18n Integration

mdBook supports multiple languages through the `mdbook-i18n-helpers` preprocessor or manual language directories.

**Approach A: Language Directories (Recommended)**

Each guide has parallel content in language subdirectories:

```
api-consumer-guide/
  book.toml              # Multi-language configuration
  en/
    SUMMARY.md
    getting-started/
      intro.md
      ...
  pt-BR/
    SUMMARY.md
    getting-started/
      intro.md
      ...
```

**`book.toml` multi-language configuration:**
```toml
[book]
title = "API Consumer Guide"
language = "en"
multilingual = true

[language.en]
name = "English"

[language.pt-BR]
name = "Português (Brasil)"

[language.pt-PT]
name = "Português (Portugal)"

[output.html]
default-theme = "light"
```

**Approach B: Fluent Preprocessor (Advanced)**

A custom mdBook preprocessor that substitutes Fluent messages:

```markdown
# { -term-glossary }

| { -term-header } | { -definition-header } |
|------------------|------------------------|
| { term-task } | { term-task-def } |
| { term-step } | { term-step-def } |
```

The preprocessor reads the current language and substitutes from `.ftl` files.

### Translation Workflow

1. **Source content** is written in English (`en/`)
2. **Extract strings** to `.ftl` files using automated tooling
3. **Translate `.ftl` files** to target languages
4. **Copy structure** from `en/` to `pt-BR/` and `pt-PT/`
5. **Replace content** with translated versions
6. **Validate** using `mdbook test` and Fluent linting

### Translation Priority

For initial release, prioritize:
1. Foundation content (glossary, core concepts, authentication)
2. API Consumer Guide (most commonly referenced externally)
3. Journey Planner Guide (primary authoring audience)
4. Journey Developer Guide (technical, less translation-sensitive)

---

## Content Categorization

### Shared Foundation Content

These files will live in `_foundation/` and be included by multiple guides:

| File | Description | Used By |
|------|-------------|---------|
| `glossary.md` | Unified terminology definitions | All guides |
| `concepts/core-execution-model.md` | Task, Step, Stage, Playbook, Journey | All guides |
| `concepts/briefs-and-packs.md` | Brief, BriefGroup, BriefPack hierarchy | All guides |
| `concepts/artifacts-and-files.md` | artifact.json, error.json, etc. | Journey Planner |
| `concepts/environment-variables.md` | OUT_DIR, IN_ARTIFACT, etc. | Journey Planner |
| `concepts/versioning-model.md` | How versioning works | Both guides |
| `authentication/overview.md` | Auth comparison and flow diagram | Both guides |
| `authentication/web-sessions.md` | NextAuth JWT details | API Consumer |
| `authentication/api-tokens.md` | Opaque PAT details | Both guides |
| `authentication/service-tokens.md` | Ed25519 service auth | API Consumer |
| `authentication/ethereum-tokens.md` | Ethereum-signed auth | API Consumer |
| `authentication/best-practices.md` | Security checklist | Both guides |
| `data-model/principal-objects.md` | Principal object definitions | Both guides |
| `data-model/versioned-objects.md` | Versioned object patterns | Both guides |
| `data-model/associative-tables.md` | Many-to-many patterns | Both guides |

### API Consumer Guide Content

These files are specific to API consumers (frontend developers, integration partners):

| File | Description | Source |
|------|-------------|--------|
| `preface.md` | API Consumer specific preface | New |
| `AGENTS.md` | Agent reading guide for API tasks | New |
| `intro.md` | API Consumer orientation | New |
| `api-patterns.md` | Batch semantics, query patterns | Move from current |
| `runtime-operations.md` | Journey-centric workflows | Move from current |
| `api-cookbook.md` | Copy-paste recipes | Move from current |
| `endpoint-reference.md` | Consolidated endpoint docs | Extract from api-patterns.md |
| `error-handling.md` | Error codes and troubleshooting | Extract from multiple |

### Journey Developer Guide Content

These files are specific to backend developers working on system internals:

| File | Description | Source |
|------|-------------|--------|
| `preface.md` | Developer-focused preface | New |
| `AGENTS.md` | Agent reading guide for dev tasks | New |
| `architecture-overview.md` | System component diagram | New |
| **Flow Engine** | | |
| `flow/overview.md` | Flow engine architecture | Extract from dapple-prompt-flow.md |
| `flow/nushell-execution.md` | Nushell execution model | Extract from dapple-prompt-flow.md |
| `flow/step-engine.md` | StepEngine internals | New |
| `flow/dataflow-integration.md` | dataflow-rs integration | New |
| `flow/security-model.md` | Nushell sandboxing | Extract from dapple-prompt-flow.md |
| **Conduit Service** | | |
| `conduit/overview.md` | Conduit architecture | Extract from dapple-prompt-conduit.md |
| `conduit/llm-backends.md` | Backend implementations | Extract from dapple-prompt-conduit.md |
| `conduit/embedding-backends.md` | Embedding services | New |
| `conduit/template-rendering.md` | Handlebars system | Extract from dapple-prompt-conduit.md |
| `conduit/overlay-system.md` | Slot visibility, ranking | New |
| **Nushell Commands** | | |
| `nushell/custom-commands.md` | Command reference | Move from custom-nu-commands.md |
| `nushell/command-authoring.md` | Writing new commands | New |
| `nushell/plugin-system.md` | Plugin architecture | New |
| **Data Layer** | | |
| `data-layer/query-patterns.md` | SQL conventions | New (from CLAUDE.md patterns) |
| `data-layer/model-patterns.md` | Rust model conventions | New (from CLAUDE.md patterns) |
| `data-layer/migration-guide.md` | Writing migrations | New |
| **Development** | | |
| `development/testing-patterns.md` | Integration tests | New (from testing-debugging.md) |
| `development/local-development.md` | Dev environment | New |
| `development/debugging.md` | Debugging techniques | New |

### Journey Planner Guide Content

These files are specific to playbook authors designing workflows and user experiences:

| File | Description | Status |
|------|-------------|--------|
| `preface.md` | Planner-focused preface | Update |
| `AGENTS.md` | Agent reading guide for authoring | Update |
| `intro.md` | Planner orientation | Update |
| **Authoring** | | |
| `authoring/composing-steps-stages.md` | Building workflows | Keep |
| `authoring/versioning.md` | Versioning practices | Keep |
| `authoring/testing-debugging.md` | Testing playbooks | Keep (simplified) |
| **Brief System** | | |
| `briefs/brief-pack-manifest.md` | Pack manifest format | Move from current |
| `briefs/brief-units.md` | UI elements | New (extracted) |
| `briefs/debrief-patterns.md` | UX patterns | New (from concepts.md) |
| **Prompts** | | |
| `prompts/prompt-authoring.md` | Writing prompts | Keep |
| `prompts/adding-prompts.md` | Adding to packs | Keep |
| `prompts/fluent-messages.md` | Localized messages | Keep |
| **Walkthroughs** | | |
| `walkthroughs/resume-walkthrough.md` | End-to-end example | Keep |
| **Reference** | | |
| `cookbook.md` | Authoring recipes | Keep |
| `authors-notes.md` | WIP notes | Keep |
| `templates.md` | Template reference | Keep |
| `appendix.md` | Appendix | Keep |
| `directives.md` | JSONLogic conditionals | Keep |
| **Examples** | | |
| `examples/simple-nu-task.md` | Task example | Keep |
| `examples/ordered-setup-step.md` | Step example | Keep |
| `examples/resume-playbook.md` | Playbook example | Keep |
| `examples/prompt-templates.md` | Prompt example | Keep |

### Content Migration Summary

| From Current Guide | To Guide | Notes |
|--------------------|----------|-------|
| `dapple-prompt-flow.md` | Journey Developer | Split into flow/ directory |
| `dapple-prompt-conduit.md` | Journey Developer | Split into conduit/ directory |
| `custom-nu-commands.md` | Journey Developer | Move to nushell/ directory |
| `nushell-task-template.md` | Journey Developer | Move to nushell/ |
| `api-patterns.md` | API Consumer | Direct move |
| `runtime-operations.md` | API Consumer | Direct move |
| `api-cookbook.md` | API Consumer | Direct move |
| `authentication.md` | Foundation | Split into auth/ directory |
| `glossary.md` | Foundation | Direct move |
| `concepts.md` | Foundation | Split into concepts/ directory |
| `brief-pack-manifest.md` | Journey Planner | Move to briefs/ |
| `composing-steps-stages.md` | Journey Planner | Move to authoring/ |
| `versioning.md` | Journey Planner | Move to authoring/ |
| `prompt-authoring.md` | Journey Planner | Move to prompts/ |
| `adding-prompts.md` | Journey Planner | Move to prompts/ |
| `fluent-messages.md` | Journey Planner | Move to prompts/ |
| `testing-debugging.md` | Both Planner & Developer | Split content |

---

## Technical Approach

### mdBook Include Directive

mdBook supports including content from other files using the `{{#include}}` directive:

```markdown
# Glossary

{{#include ../_foundation/glossary.md}}
```

This approach:
- Avoids content duplication
- Maintains single source of truth
- Allows guide-specific context around included content

### Include Patterns

**Full file include:**
```markdown
{{#include ../_foundation/glossary.md}}
```

**Anchored include (specific section):**
```markdown
{{#include ../_foundation/concepts/core-execution-model.md:task}}
```

Requires anchor tags in source:
```markdown
# Core Execution Model

<!-- ANCHOR: task -->
### Task
A Task is the smallest unit of work...
<!-- ANCHOR_END: task -->
```

**Partial include with line ranges:**
```markdown
{{#include ../_foundation/authentication/overview.md:5:50}}
```

### Guide-Specific Customization

When guides need to add context around shared content:

```markdown
# Authentication

> **For API Consumers:** This section covers the four authentication
> methods available. Most API consumers should use
> [Opaque PATs](#opaque-personal-access-tokens-pat) for programmatic access.

{{#include ../_foundation/authentication/overview.md}}

## API Consumer Specific Notes

When building integrations...
```

### Book Configuration

Each guide has its own `book.toml`:

```toml
# api-consumer-guide/book.toml
[book]
title = "API Consumer Guide"
authors = ["dapple-prompt team"]
language = "en"
version = "0.1.0"
src = "."
description = "Guide for consuming the dapple-prompt REST API"

[build]
build-dir = "book"
create-missing = false

[output.html]
default-theme = "light"
preferred-dark-theme = "navy"
git-repository-url = "https://github.com/soulstompp/dapple-prompt"
edit-url-template = "https://github.com/soulstompp/dapple-prompt/edit/main/docs/guides/api-consumer-guide/{path}"
site-url = "/api-consumer-guide/"

[preprocessor.mermaid]
command = "mdbook-mermaid"
```

### SUMMARY.md Structure

**API Consumer Guide SUMMARY.md:**
```markdown
# Summary

[Preface](./preface.md)
[Agent Reading Guide](./AGENTS.md)

- [Introduction](./getting-started/intro.md)

# Getting Started

- [Glossary & Terms](./getting-started/glossary.md)
- [Core Concepts](./getting-started/concepts.md)
- [Authentication](./getting-started/authentication.md)

# API Reference

- [API Patterns & Semantics](./api-patterns.md)
- [Endpoint Reference](./endpoint-reference.md)

# Runtime Operations

- [Journey-Centric Workflows](./runtime-operations.md)

# Recipes & Examples

- [API Cookbook](./api-cookbook.md)
- [Error Handling](./error-handling.md)
```

**Journey Developer Guide SUMMARY.md:**
```markdown
# Summary

[Preface](./preface.md)
[Agent Reading Guide](./AGENTS.md)

- [Introduction](./getting-started/intro.md)

# Getting Started

- [Glossary & Terms](./getting-started/glossary.md)
- [Core Concepts](./getting-started/concepts.md)
- [Architecture Overview](./getting-started/architecture-overview.md)

# Flow Engine

- [Overview](./flow/overview.md)
- [Nushell Execution Model](./flow/nushell-execution.md)
- [StepEngine Internals](./flow/step-engine.md)
- [Dataflow Integration](./flow/dataflow-integration.md)
- [Security Model](./flow/security-model.md)

# Conduit Service

- [Overview](./conduit/overview.md)
- [LLM Backends](./conduit/llm-backends.md)
- [Embedding Backends](./conduit/embedding-backends.md)
- [Template Rendering](./conduit/template-rendering.md)
- [Overlay System](./conduit/overlay-system.md)

# Nushell Commands

- [Custom Commands Reference](./nushell/custom-commands.md)
- [Command Authoring](./nushell/command-authoring.md)
- [Plugin System](./nushell/plugin-system.md)

# Data Layer

- [Query Patterns](./data-layer/query-patterns.md)
- [Model Patterns](./data-layer/model-patterns.md)
- [Migration Guide](./data-layer/migration-guide.md)

# Development

- [Testing Patterns](./development/testing-patterns.md)
- [Local Development](./development/local-development.md)
- [Debugging](./development/debugging.md)
```

**Journey Planner Guide SUMMARY.md:**
```markdown
# Summary

[Preface](./preface.md)
[Agent Reading Guide](./AGENTS.md)

- [Introduction](./getting-started/intro.md)

# Getting Started

- [Glossary & Terms](./getting-started/glossary.md)
- [Core Concepts](./getting-started/concepts.md)
- [Directives and JSONLogic](./getting-started/directives.md)

# Authoring Playbooks

- [Composing Steps & Stages](./authoring/composing-steps-stages.md)
- [Versioning](./authoring/versioning.md)
- [Testing & Debugging](./authoring/testing-debugging.md)

# Brief System

- [Brief Pack Manifest](./briefs/brief-pack-manifest.md)
- [Brief Units](./briefs/brief-units.md)
- [Debrief UX Patterns](./briefs/debrief-patterns.md)

# Prompts & LLM

- [Prompt Authoring](./prompts/prompt-authoring.md)
- [Adding Prompts & Packs](./prompts/adding-prompts.md)
- [Fluent Messages](./prompts/fluent-messages.md)

# Walkthroughs

- [Build a Resume Walkthrough](./walkthroughs/resume-walkthrough.md)

# Reference

- [Cookbook](./cookbook.md)
- [Authors Notes & WIP](./authors-notes.md)
- [Templates](./templates.md)
- [Appendix](./appendix.md)

# Examples

- [Simple Nushell Task](./examples/simple-nu-task.md)
- [Ordered Setup Step](./examples/ordered-setup-step.md)
- [Resume Playbook](./examples/resume-playbook.md)
- [Prompt Templates](./examples/prompt-templates.md)
```

---

## Migration Strategy

### Phase 1: Create Foundation Layer and Locales

1. Create `docs/guides/_foundation/` directory structure
2. Create `docs/guides/_locales/` directory structure with `en/`, `pt-BR/`, `pt-PT/`
3. Extract shared content from current guide into foundation files:
   - Split `glossary.md` into foundation (keep full content)
   - Split `concepts.md` into modular foundation files
   - Split `authentication.md` into modular foundation files
4. Add anchor tags to foundation files for partial includes
5. Create initial Fluent `.ftl` files for glossary terms and common strings
6. Verify foundation files build correctly as standalone markdown

### Phase 2: Create API Consumer Guide

1. Create `docs/guides/api-consumer-guide/` directory structure with `en/` subdirectory
2. Create new `book.toml` with multilingual configuration
3. Create `SUMMARY.md` for API Consumer focus
4. Create guide-specific files:
   - `preface.md` - API Consumer focused
   - `AGENTS.md` - API task reading guide
   - `getting-started/intro.md` - API Consumer orientation
   - `getting-started/glossary.md` - Include from foundation
   - `getting-started/concepts.md` - Curated includes
   - `getting-started/authentication.md` - Include from foundation
5. Move/copy existing content:
   - `api-patterns.md` from current guide
   - `runtime-operations.md` from current guide
   - `api-cookbook.md` from current guide
6. Create new reference files:
   - `endpoint-reference.md` - Consolidated from api-patterns.md
   - `error-handling.md` - Extracted from multiple files
7. Update internal links to use new paths
8. Test build with `mdbook build`

### Phase 3: Create Journey Developer Guide

1. Create `docs/guides/journey-developer-guide/` directory structure with `en/` subdirectory
2. Create new `book.toml` with multilingual configuration
3. Create `SUMMARY.md` for developer focus
4. Create guide-specific files:
   - `preface.md` - Developer focused
   - `AGENTS.md` - Dev task reading guide
   - `architecture-overview.md` - System component diagram
5. Extract and organize Flow Engine content:
   - Split `dapple-prompt-flow.md` into `flow/` directory files
   - Add new files: `step-engine.md`, `dataflow-integration.md`
6. Extract and organize Conduit content:
   - Split `dapple-prompt-conduit.md` into `conduit/` directory files
   - Add new files: `embedding-backends.md`, `overlay-system.md`
7. Create Nushell section:
   - Move `custom-nu-commands.md` to `nushell/custom-commands.md`
   - Move `nushell-task-template.md` to `nushell/` or Journey Planner
   - Add new files: `command-authoring.md`, `plugin-system.md`
8. Create Data Layer section (from CLAUDE.md patterns):
   - `query-patterns.md`, `model-patterns.md`, `migration-guide.md`
9. Create Development section:
   - Extract from `testing-debugging.md`
   - Add `local-development.md`, `debugging.md`
10. Update internal links
11. Test build with `mdbook build`

### Phase 4: Refactor Journey Planner Guide

1. Create `docs/guides/journey-planner-guide/` directory structure with `en/` subdirectory
2. Create new `book.toml` with multilingual configuration
3. Create new `SUMMARY.md` reflecting reorganization
4. Update guide-specific files to use foundation includes:
   - `getting-started/glossary.md` - Include from foundation
   - `getting-started/concepts.md` - Include + authoring content
5. Organize content into subdirectories:
   - `authoring/` - Workflow composition files
   - `briefs/` - Brief system files
   - `prompts/` - Prompt-related files
   - `walkthroughs/` - Walkthrough files
   - `examples/` - Example files
6. Remove content that moved to other guides:
   - Runtime internals → Journey Developer
   - API patterns → API Consumer
7. Update internal links to use new paths
8. Test build with `mdbook build`

### Phase 5: Setup Multilingual Infrastructure

1. Create translation workflow documentation
2. Set up language directories for each guide
3. Create placeholder files in `pt-BR/` and `pt-PT/`
4. Configure mdBook for language switching
5. Add translation status tracking (what % is translated)

### Phase 6: Update References and Deprecate Old

1. Update links in CLAUDE.md that reference documentation
2. Update any source code documentation links
3. Update CI/CD scripts for new documentation paths
4. Update any external documentation links
5. Create redirect rules for old URLs (if deployed)
6. Add deprecation notice to old guide location
7. After verification period, remove old guide
8. Update `.gitignore` if needed

---

## Files to Create

### Foundation and Locales

| File | Description |
|------|-------------|
| `docs/guides/_foundation/glossary.md` | Unified glossary |
| `docs/guides/_foundation/concepts/core-execution-model.md` | Task, Step, Stage, etc. |
| `docs/guides/_foundation/concepts/briefs-and-packs.md` | Brief hierarchy |
| `docs/guides/_foundation/concepts/artifacts-and-files.md` | File contracts |
| `docs/guides/_foundation/concepts/environment-variables.md` | Runtime env vars |
| `docs/guides/_foundation/concepts/versioning-model.md` | Versioning patterns |
| `docs/guides/_foundation/authentication/overview.md` | Auth overview |
| `docs/guides/_foundation/authentication/web-sessions.md` | NextAuth JWT |
| `docs/guides/_foundation/authentication/api-tokens.md` | Opaque PAT |
| `docs/guides/_foundation/authentication/service-tokens.md` | Ed25519 |
| `docs/guides/_foundation/authentication/ethereum-tokens.md` | Ethereum |
| `docs/guides/_foundation/authentication/best-practices.md` | Security |
| `docs/guides/_foundation/data-model/principal-objects.md` | Principals |
| `docs/guides/_foundation/data-model/versioned-objects.md` | Versioned |
| `docs/guides/_foundation/data-model/associative-tables.md` | Associative |
| `docs/guides/_locales/en/common.ftl` | English common strings |
| `docs/guides/_locales/en/glossary.ftl` | English glossary terms |
| `docs/guides/_locales/en/errors.ftl` | English error messages |
| `docs/guides/_locales/en/guide-titles.ftl` | English guide titles |
| `docs/guides/_locales/pt-BR/common.ftl` | Brazilian Portuguese common |
| `docs/guides/_locales/pt-BR/glossary.ftl` | Brazilian Portuguese glossary |
| `docs/guides/_locales/pt-BR/errors.ftl` | Brazilian Portuguese errors |
| `docs/guides/_locales/pt-BR/guide-titles.ftl` | Brazilian Portuguese titles |
| `docs/guides/_locales/pt-PT/common.ftl` | European Portuguese common |
| `docs/guides/_locales/pt-PT/glossary.ftl` | European Portuguese glossary |
| `docs/guides/_locales/pt-PT/errors.ftl` | European Portuguese errors |
| `docs/guides/_locales/pt-PT/guide-titles.ftl` | European Portuguese titles |

### API Consumer Guide

| File | Description |
|------|-------------|
| `docs/guides/api-consumer-guide/book.toml` | Book config (multilingual) |
| `docs/guides/api-consumer-guide/SUMMARY.md` | Table of contents |
| `docs/guides/api-consumer-guide/preface.md` | API Consumer preface |
| `docs/guides/api-consumer-guide/AGENTS.md` | Agent guide |
| `docs/guides/api-consumer-guide/getting-started/intro.md` | Introduction |
| `docs/guides/api-consumer-guide/getting-started/glossary.md` | Include wrapper |
| `docs/guides/api-consumer-guide/getting-started/concepts.md` | Include wrapper |
| `docs/guides/api-consumer-guide/getting-started/authentication.md` | Include wrapper |
| `docs/guides/api-consumer-guide/endpoint-reference.md` | Consolidated reference |
| `docs/guides/api-consumer-guide/error-handling.md` | Error guide |

### Journey Developer Guide

| File | Description |
|------|-------------|
| `docs/guides/journey-developer-guide/book.toml` | Book config (multilingual) |
| `docs/guides/journey-developer-guide/SUMMARY.md` | Table of contents |
| `docs/guides/journey-developer-guide/preface.md` | Developer preface |
| `docs/guides/journey-developer-guide/AGENTS.md` | Agent guide |
| `docs/guides/journey-developer-guide/getting-started/intro.md` | Introduction |
| `docs/guides/journey-developer-guide/getting-started/glossary.md` | Include wrapper |
| `docs/guides/journey-developer-guide/getting-started/concepts.md` | Include wrapper |
| `docs/guides/journey-developer-guide/getting-started/architecture-overview.md` | System diagram |
| `docs/guides/journey-developer-guide/flow/overview.md` | Flow engine overview |
| `docs/guides/journey-developer-guide/flow/nushell-execution.md` | Nushell execution |
| `docs/guides/journey-developer-guide/flow/step-engine.md` | StepEngine internals |
| `docs/guides/journey-developer-guide/flow/dataflow-integration.md` | dataflow-rs |
| `docs/guides/journey-developer-guide/flow/security-model.md` | Sandboxing |
| `docs/guides/journey-developer-guide/conduit/overview.md` | Conduit overview |
| `docs/guides/journey-developer-guide/conduit/llm-backends.md` | Backend implementations |
| `docs/guides/journey-developer-guide/conduit/embedding-backends.md` | Embedding services |
| `docs/guides/journey-developer-guide/conduit/template-rendering.md` | Handlebars |
| `docs/guides/journey-developer-guide/conduit/overlay-system.md` | Overlays |
| `docs/guides/journey-developer-guide/nushell/custom-commands.md` | Command reference |
| `docs/guides/journey-developer-guide/nushell/command-authoring.md` | Writing commands |
| `docs/guides/journey-developer-guide/nushell/plugin-system.md` | Plugins |
| `docs/guides/journey-developer-guide/data-layer/query-patterns.md` | SQL patterns |
| `docs/guides/journey-developer-guide/data-layer/model-patterns.md` | Rust patterns |
| `docs/guides/journey-developer-guide/data-layer/migration-guide.md` | Migrations |
| `docs/guides/journey-developer-guide/development/testing-patterns.md` | Test patterns |
| `docs/guides/journey-developer-guide/development/local-development.md` | Local dev |
| `docs/guides/journey-developer-guide/development/debugging.md` | Debugging |

### Journey Planner Guide

| File | Description |
|------|-------------|
| `docs/guides/journey-planner-guide/book.toml` | Book config (multilingual) |
| `docs/guides/journey-planner-guide/SUMMARY.md` | Updated TOC |
| `docs/guides/journey-planner-guide/preface.md` | Planner preface |
| `docs/guides/journey-planner-guide/AGENTS.md` | Agent guide |
| `docs/guides/journey-planner-guide/getting-started/intro.md` | Introduction |
| `docs/guides/journey-planner-guide/getting-started/glossary.md` | Include wrapper |
| `docs/guides/journey-planner-guide/getting-started/concepts.md` | Include wrapper |
| `docs/guides/journey-planner-guide/briefs/brief-units.md` | UI elements (new) |
| `docs/guides/journey-planner-guide/briefs/debrief-patterns.md` | UX patterns (new) |

## Files to Move

### To Foundation (Shared)

| Current Location | New Location | Notes |
|------------------|--------------|-------|
| `.../journey-planners-guide/authentication.md` | Split to `_foundation/authentication/` | Split into 6 files |
| `.../journey-planners-guide/glossary.md` | `_foundation/glossary.md` | Keep complete |
| `.../journey-planners-guide/concepts.md` | Split to `_foundation/concepts/` | Split into 5 files |

### To API Consumer Guide

| Current Location | New Location | Notes |
|------------------|--------------|-------|
| `.../journey-planners-guide/api-patterns.md` | `api-consumer-guide/api-patterns.md` | Direct move |
| `.../journey-planners-guide/runtime-operations.md` | `api-consumer-guide/runtime-operations.md` | Direct move |
| `.../journey-planners-guide/api-cookbook.md` | `api-consumer-guide/api-cookbook.md` | Direct move |

### To Journey Developer Guide

| Current Location | New Location | Notes |
|------------------|--------------|-------|
| `.../journey-planners-guide/dapple-prompt-flow.md` | Split to `journey-developer-guide/flow/` | Split into 5 files |
| `.../journey-planners-guide/dapple-prompt-conduit.md` | Split to `journey-developer-guide/conduit/` | Split into 5 files |
| `.../journey-planners-guide/custom-nu-commands.md` | `journey-developer-guide/nushell/custom-commands.md` | Direct move |
| `.../journey-planners-guide/nushell-task-template.md` | `journey-developer-guide/nushell/task-template.md` | Direct move |

### To Journey Planner Guide

| Current Location | New Location | Notes |
|------------------|--------------|-------|
| `.../journey-planners-guide/composing-steps-stages.md` | `journey-planner-guide/authoring/composing-steps-stages.md` | Direct move |
| `.../journey-planners-guide/versioning.md` | `journey-planner-guide/authoring/versioning.md` | Direct move |
| `.../journey-planners-guide/testing-debugging.md` | `journey-planner-guide/authoring/testing-debugging.md` | Simplified version |
| `.../journey-planners-guide/brief-pack-manifest.md` | `journey-planner-guide/briefs/brief-pack-manifest.md` | Direct move |
| `.../journey-planners-guide/prompt-authoring.md` | `journey-planner-guide/prompts/prompt-authoring.md` | Direct move |
| `.../journey-planners-guide/adding-prompts.md` | `journey-planner-guide/prompts/adding-prompts.md` | Direct move |
| `.../journey-planners-guide/fluent-messages.md` | `journey-planner-guide/prompts/fluent-messages.md` | Direct move |
| `.../journey-planners-guide/resume-walkthrough.md` | `journey-planner-guide/walkthroughs/resume-walkthrough.md` | Direct move |
| `.../journey-planners-guide/directives.md` | `journey-planner-guide/getting-started/directives.md` | Direct move |
| `.../journey-planners-guide/examples/*` | `journey-planner-guide/examples/` | Direct move |
| `.../journey-planners-guide/cookbook.md` | `journey-planner-guide/cookbook.md` | Direct move |
| `.../journey-planners-guide/authors-notes.md` | `journey-planner-guide/authors-notes.md` | Direct move |
| `.../journey-planners-guide/templates.md` | `journey-planner-guide/templates.md` | Direct move |
| `.../journey-planners-guide/appendix.md` | `journey-planner-guide/appendix.md` | Direct move |
| `.../journey-planners-guide/CHANGES.md` | `journey-planner-guide/CHANGES.md` | Direct move |

---

## Future Guide Considerations

The proposed architecture supports adding new guides with minimal effort:

### Current Guide Matrix

| Guide | Primary Audience | Focus |
|-------|------------------|-------|
| API Consumer Guide | Frontend devs, integrators | REST API consumption |
| Journey Developer Guide | Backend developers | System internals, extending platform |
| Journey Planner Guide | Playbook authors | Workflow design, user experience |

### Potential Future Guides

1. **Administrator Guide** - Platform administration, user management, billing
2. **Operator Guide** - Deployment, monitoring, scaling, K8s configuration
3. **SDK Reference** - Auto-generated client library documentation
4. **Quick Start Guide** - 15-minute getting-started tutorial
5. **Migration Guide** - Version upgrade and data migration

### Adding a New Guide

1. Create `docs/guides/{guide-name}/` directory
2. Create `en/` subdirectory for English source content
3. Copy `book.toml` template and customize (include multilingual config)
4. Create `SUMMARY.md` with guide-specific structure
5. Create guide-specific content files
6. Use `{{#include}}` for foundation content
7. Add build configuration to CI/CD
8. Create placeholder directories for `pt-BR/` and `pt-PT/`

### Foundation Extension Pattern

When new guides need shared content:

1. Add content to `_foundation/` with appropriate anchors
2. Update existing guide include wrappers if needed
3. Document the new foundation content in this plan
4. Add Fluent strings to `_locales/en/*.ftl` files
5. Update translation status tracker

### Cross-Guide Linking

For linking between guides:
```markdown
See the [API Consumer Guide](../api-consumer-guide/runtime-operations.md)
for runtime workflow details.
```

### Translation Workflow for New Guides

1. Create guide content in `en/` directory
2. Extract translatable strings to `.ftl` files
3. Mark guide as "English only" in translation status
4. Request translations when guide is stable
5. Create `pt-BR/` and `pt-PT/` directories
6. Copy English content, replace with translations
7. Update translation status to complete

---

## Verification

### Build Verification

```bash
# Build all guides (English)
cd docs/guides/api-consumer-guide && mdbook build
cd docs/guides/journey-developer-guide && mdbook build
cd docs/guides/journey-planner-guide && mdbook build

# Check for broken links
mdbook test docs/guides/api-consumer-guide
mdbook test docs/guides/journey-developer-guide
mdbook test docs/guides/journey-planner-guide

# Validate Fluent files
fluent-syntax-lint docs/guides/_locales/en/*.ftl
fluent-syntax-lint docs/guides/_locales/pt-BR/*.ftl
fluent-syntax-lint docs/guides/_locales/pt-PT/*.ftl
```

### Content Verification

1. All glossary terms accessible from all three guides
2. Authentication content complete in API Consumer Guide
3. System internals content complete in Journey Developer Guide
4. Authoring content complete in Journey Planner Guide
5. No content duplication between guides (use includes)
6. All internal links resolve correctly
7. All include directives resolve correctly
8. Mermaid diagrams render correctly
9. Code blocks syntax highlight correctly

### Multilingual Verification

1. All Fluent files parse without errors
2. Glossary terms have translations in pt-BR and pt-PT
3. Language switcher works correctly
4. Fallback to English works for untranslated content
5. RTL languages render correctly (if added later)

### Manual Review Checklist

- [ ] API Consumer Guide covers all authentication methods
- [ ] API Consumer Guide includes complete endpoint reference
- [ ] Journey Developer Guide covers flow engine internals
- [ ] Journey Developer Guide covers conduit service
- [ ] Journey Developer Guide covers nushell commands
- [ ] Journey Planner Guide retains all authoring content
- [ ] Journey Planner Guide covers brief system
- [ ] Foundation glossary contains all terms from all audiences
- [ ] Foundation concepts cover data model completely
- [ ] All three guides have appropriate AGENTS.md for LLM consumers
- [ ] Cross-references between guides work correctly
- [ ] Search works within each guide
- [ ] Table of contents reflects logical structure
- [ ] Language switcher appears on all pages
- [ ] Fluent messages load correctly per language

---

## Open Questions and Risks

### Open Questions

1. **URL Structure** - Should guides be deployed to separate subdomains or paths?
   - Option A: `docs.example.com/api-consumer-guide/`
   - Option B: `api-docs.example.com/`
   - Recommendation: Use paths for simplicity with language prefix (`/en/api-consumer-guide/`)

2. **Versioning** - Should guides be versioned independently or together?
   - Recommendation: Version together initially, consider separating if release cadence diverges

3. **Language URL Pattern** - How should languages be represented in URLs?
   - Option A: `/en/api-consumer-guide/`, `/pt-br/api-consumer-guide/`
   - Option B: `/api-consumer-guide/en/`, `/api-consumer-guide/pt-br/`
   - Recommendation: Option A (language first) for consistent switching

4. **Fluent Integration Depth** - Should Fluent be used for inline content or just UI strings?
   - Option A: Full content in `.ftl` files (complex, powerful)
   - Option B: Only UI strings in `.ftl`, content as separate `.md` files (simpler)
   - Recommendation: Option B for initial implementation, consider Option A for glossary

5. **Search** - Should there be cross-guide search capability?
   - Recommendation: Each guide has its own search; consider unified search in future

6. **Translation Memory** - Should we use a translation memory tool?
   - Recommendation: Consider Weblate or Crowdin for community translations later

### Risks

1. **Broken Links** - Migration may break existing links from external sources
   - Mitigation: Implement redirects for old URLs

2. **Include Complexity** - Heavy use of includes may make editing confusing
   - Mitigation: Document include patterns clearly, use minimal nesting

3. **Build Time** - Three books × three languages = 9 build targets
   - Mitigation: Parallelize builds in CI/CD, consider incremental builds

4. **Consistency Drift** - Guide-specific content may drift from foundation
   - Mitigation: Regular cross-guide reviews, automated link checking

5. **Translation Lag** - English content may update faster than translations
   - Mitigation: Mark untranslated content, show "English fallback" notice

6. **Fluent Complexity** - Fluent syntax has a learning curve
   - Mitigation: Start with simple messages, document patterns, provide examples

---

## Implementation Notes

### For Implementation Agents

1. Start with Phase 1 (Foundation Layer + Locales) as it has no dependencies
2. Phase 2 (API Consumer) and Phase 3 (Journey Developer) can be done in parallel
3. Phase 4 (Journey Planner) depends on Phases 2 and 3 to identify what content moves where
4. Test each phase independently before proceeding
5. Use `mdbook serve` for live preview during development
6. Run `mdbook test` after each major change
7. Validate Fluent files after any `.ftl` changes

### Key mdBook Commands

```bash
# Initialize new book
mdbook init docs/guides/api-consumer-guide

# Build book
mdbook build docs/guides/api-consumer-guide

# Serve with live reload
mdbook serve docs/guides/api-consumer-guide

# Test for broken links
mdbook test docs/guides/api-consumer-guide
```

### Include Directive Gotchas

1. Paths are relative to the file containing the include
2. Include paths must use forward slashes on all platforms
3. Anchor names are case-sensitive
4. Missing includes cause build errors (fail-fast)
5. Circular includes are not detected until build time

### Fluent Integration Notes

1. **Message IDs** use kebab-case: `term-brief-group`, `error-auth-failed`
2. **Attributes** use dot notation: `term-task.definition`
3. **Placeholders** use curly braces: `{ $count } items`
4. **Selectors** for pluralization: `{ $count -> [one] item *[other] items }`

**Basic Fluent patterns:**

```ftl
# Simple message
term-task = Task

# Message with attribute
term-task =
    .short = Task
    .definition = A single unit of work implemented as a small script.

# Message with placeholder
items-count = { $count } items found

# Plural selection
items-count = { $count ->
    [one] { $count } item found
   *[other] { $count } items found
}
```

### Translation Tracking

Maintain a translation status file:

```markdown
# Translation Status

| Guide | en | pt-BR | pt-PT |
|-------|:--:|:-----:|:-----:|
| Foundation | ✅ | 🔄 80% | 🔄 60% |
| API Consumer | ✅ | 🔄 50% | ❌ 0% |
| Journey Developer | ✅ | ❌ 0% | ❌ 0% |
| Journey Planner | ✅ | 🔄 30% | ❌ 0% |

Legend: ✅ Complete | 🔄 In Progress | ❌ Not Started
```
