# Preface

This document is a section-by-section comparative analysis of the paper **"Everything is Context: Agentic File System Abstraction for Context Engineering"** (Xu et al., arXiv:2512.05470, December 2025) against the dapple-prompt architecture.

## Citation

> Xu, Y. et al. (2025). *Everything is Context: Agentic File System Abstraction for Context Engineering.* arXiv:2512.05470.

## How to Read This Document

Each chapter follows the paper's structure and uses two lenses:

- **What the paper says** — summarizes the section's key claims and proposals
- **What dapple-prompt does** — maps those claims to our concrete implementation

These two lenses are followed by a **Comparison** section highlighting alignment, divergence, and potential lessons, and a **TLDR** distilling the chapter to its essentials.

## Quick Summary of dapple-prompt

For readers unfamiliar with the system, dapple-prompt is a Rust monorepo providing:

- **Guide authoring and ingest** — Authors write structured content (markdown directives, TOML metadata, Fluent localization files). The parser (`dapple-parser`) produces a `ParsedGuide`, which is transformed into an `IngestPlan` and materialized atomically into PostgreSQL.
- **Content-addressable versioning** — Every versioned object carries a SHA-1 `content_hash` on `rev.versioned_objects`. Playbooks, stages, and steps use a Merkle pattern where parent hashes include sorted child hashes, so any descendant change propagates upward.
- **Embedding and search** — Guide content is embedded via ModernBERT/ONNX into pgvector, enabling natural language search over guide blocks with tag and locale filtering.
- **Prompt execution** — The conduit service routes prompts to LLM backends (Anthropic, Mistral) with optional RAG context injection from guide embeddings.
- **Workflow orchestration** — The flow engine executes playbook dispatches step-by-step with Nushell scripting, JSONLogic directive evaluation, retry/branch/skip logic, and full audit trails.
- **Localization** — All localizable text lives in Fluent `.ftl` files. Guide structure is locale-independent; text is resolved per-locale at render or prompt time.

The system is organized as a Cargo workspace with crates for data access (`dapple-data`), parsing (`dapple-parser`), rendering (`dapple-render`), the REST API (`dapple-prompt-api`), the LLM conduit (`dapple-prompt-conduit`), and the flow engine (`dapple-prompt-flow`).
