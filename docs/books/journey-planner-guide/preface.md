# Preface

<div style="text-align: center; margin: 1em 0 2em 0;">
<strong style="font-size: 1.2em;">Version 0.3.2</strong><br/>
<a href="./CHANGES.md">What's new in this version?</a>
</div>

## Version
0.3.2

## Purpose

This guide, "Journey Planners Guide", serves **Guide Authors and Workflow Designers** -- the people who create the structured content and workflows that power the dapple-prompt platform.

It covers everything from writing your first guide directory to designing complex multi-stage workflows with directives, Nushell tasks, and inference profiles.

> **For programmatic API consumption** (SDK methods, HTTP endpoints, runtime operations), see the **[API Consumer Guide](../api-consumer-guide/preface.md)**.

## Audiences

This guide serves two audiences, marked at the top of each chapter:

| Icon | Audience | Focus |
|------|----------|-------|
| :wrench: | **Prompt Engineers** | Guide authoring, Nushell scripting, LLM configuration, inference profiles |
| :clipboard: | **Product** | High-level guidance, checklists, acceptance criteria |

Chapters marked with multiple icons are relevant to all listed audiences. Technical chapters include a note when Product readers can skip them.

## How to Use This Guide

### For Guide Authors

- **Start here:** Read the [Intro](./intro.md) and [Glossary](./glossary.md) for orientation
- **Directory layout:** Read [Guide Directory Structure](./guide-directory-structure.md) to understand how guides are organized on disk
- **Metadata:** Read [guide.toml Reference](./guide-toml-reference.md) for guide metadata configuration
- **Content:** Read [Directive Blocks](./directive-blocks.md) to learn how stages, steps, tasks, and other directives are authored in markdown
- **Localization:** Read [Fluent Localization](./fluent-localization.md) for multi-locale support
- **Walkthrough:** Follow the [Resume Guide Walkthrough](./resume-guide-walkthrough.md) for a complete authoring example

### For Workflow Designers

- **Flow control:** Read [Directives and JSONLogic](./directives.md) for declarative flow control
- **Nushell scripting:** Read [Writing Nushell Tasks](./writing-nushell-tasks.md) for task implementation
- **Runtime:** Read [Task Execution Runtime](./dapple-prompt-flow.md) for execution behavior
- **LLM routing:** Read [Prompt Execution and Routing](./dapple-prompt-conduit.md) for prompt delivery

### For Everyone

- **Terminology:** The [Glossary](./glossary.md) defines every term used across the guide
- **Troubleshooting:** Check [Testing & Debugging](./testing-debugging.md) for diagnostics and common issues

## Conventions

- **File references** point to source files in the repository (e.g., `crates/dapple-data/assets/sql/...`).
- **Audience callouts** appear at the top of each chapter.
- **Code blocks** use syntax highlighting: `nushell`, `json`, `sql`, `rust`.
- **Review markers** (`<!-- REVIEW: ... -->`) indicate sections needing verification.

## Status

**Current Version: 0.3.2** - See [What's Changed](./CHANGES.md) for details.

**New in 0.3.2:**
- Ingest pipeline now documents full prompt entity wiring (prompts, task-prompts, inference profile configs)
- Inference profile Fluent attributes for model selection: `.lineage`, `.tier`, `.release`
- Claude 4.6 lineage added to model taxonomy examples
- Conduit LLM backends table corrected with lineage keys

**Still in progress:**
- European Portuguese translations for new content
- Advanced patterns chapter
- Performance optimization guide

See [Changes](./CHANGES.md) for version history and what's new for human readers.
