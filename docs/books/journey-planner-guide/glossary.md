# Glossary & Terms

> **Audience:** Prompt Engineers · Product · Agents

This glossary provides quick definitions for key terms. Each term links to a detailed section in the [Concepts](./concepts.md) chapter.

## Guide Authoring

| Term | Definition |
|------|------------|
| **Guide Directory** | The on-disk structure containing `guide.toml`, `SUMMARY.md`, chapters, `locales/`, and `assets/` |
| **guide.toml** | Metadata file at the root of a guide directory defining title, description, author, classification, version, card, and deprecation |
| **Directive Block** | A fenced content block in guide markdown (`:::type { attrs } ... :::`) that maps to a platform entity |
| **IngestPlan** | The pure transform output from `plan_guide()` containing all derived objects and metadata |
| **Derived Objects** | Entities automatically extracted from directive blocks during ingest: tasks, playbooks, stages, steps, inference profiles, brief packs |
| **Tarball Ingest** | The process of uploading a `.tar.gz` guide directory to the API for parsing and materialization |
| **Dry Run** | Validation-only ingest mode (`dry_run=true`) that returns parsed results without writing to the database |
| **Shared Content** | Reusable content libraries (`_shared/common/` and `_shared/internal/`) that multiple guides can reference |
| **Fluent Localization** | The `.ftl` file system for providing localized text for guide content across multiple locales |
| **Dapple SDK** | Typed Rust HTTP client (`dapple-sdk` crate) providing ergonomic access to all API resources |

{{#include ../_foundation/glossary.md}}
