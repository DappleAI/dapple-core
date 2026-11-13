# Agent Reading Guide — Context Engineering Review

## What this book is for

This is a **comparative essay**, not a how-to guide. It walks the paper *Everything is Context: Agentic File System Abstraction for Context Engineering* (Xu et al., arXiv:2512.05470, December 2025) section-by-section and contrasts each claim with how dapple-prompt actually works.

Read this only if you are:
- working on context engineering, RAG, persistent memory, or scratchpad design and want to know what the literature says vs. what we already do;
- evaluating an architectural change against published prior art;
- preparing a response to that paper or a similar one.

If your task is to author a Guide, call the API, extend the platform, or implement billing/audit features, **stop here and read the relevant book instead** — see the table below.

## You probably want a different book

| Goal | Read instead |
|------|--------------|
| Author a Guide directory | [`../journey-planner-guide/AGENTS.md`](../journey-planner-guide/AGENTS.md) |
| Call the API or use the SDK | [`../api-consumer-guide/AGENTS.md`](../api-consumer-guide/AGENTS.md) |
| Extend the parser, data layer, conduit, or flow engine | [`../journey-builder-guide/AGENTS.md`](../journey-builder-guide/AGENTS.md) |
| Implement audit / billing infrastructure | [`../journey-auditor-guide/AGENTS.md`](../journey-auditor-guide/AGENTS.md) (mostly WIP) |
| Look up shared concepts and glossary | [`../_foundation/`](../_foundation/) |

## Reading order

1. `preface.md` — what the paper is, what this essay is, a quick summary of dapple-prompt for readers unfamiliar with it.
2. `01-introduction.md` through `07-conclusion.md` — chapters mirror the paper's structure.

Each chapter uses the same lenses:
- **What the paper says.**
- **What dapple-prompt does.**
- **Comparison** — alignment, divergence, lessons.
- **TLDR.**

If you only have a few minutes, read the TLDR of `06-implementation-aigne.md` (the closest direct comparison) and the `07-conclusion.md`.

## What this essay is *not*

- It is not the source of truth for how dapple-prompt currently works. The chapters describe the system as it stood when the essay was written and may drift; for current behavior verify against `crates/*/AGENTS.md` and the platform code.
- It is not engineering documentation. Don't cite TLDR claims as architecture decisions without checking the underlying code.
- It is not a paper review for publication; it is internal analysis.

## Cross-book pointers

| You want to… | Go to |
|--------------|-------|
| Understand the actual RAG pipeline | [`../journey-builder-guide/conduit/embedding-backends.md`](../journey-builder-guide/conduit/embedding-backends.md) and `crates/dapple-prompt-conduit/AGENTS.md` |
| Understand the actual ingest pipeline | [`../journey-planner-guide/ingest-pipeline.md`](../journey-planner-guide/ingest-pipeline.md) |
| See platform-wide rules of the road | [`../README.md`](../README.md) |
