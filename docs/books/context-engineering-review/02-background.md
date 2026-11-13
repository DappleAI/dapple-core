# Background — LLM-as-OS and Related Work

## What the Paper Says

The paper surveys the landscape of agentic AI infrastructure through several lenses:

**LLM-as-OS paradigm.** Drawing on AIOS (Mei et al., 2024), the paper proposes treating the LLM as an operating system kernel. Just as an OS manages processes, memory, and I/O, an "LLM OS" should manage agents, context, and tool access. The file system becomes the natural abstraction layer — everything is a file, everything has a path, everything supports read/write/list operations.

**Memory hierarchies.** The paper reviews MemGPT's tiered memory (main context vs. external storage with page-in/page-out), mem0's embedding-based memory, and Zep/Graphiti's knowledge-graph memory. Each solves a piece of the persistence problem but none provides a unified foundation.

**Framework fragmentation.** LangChain, AutoGen, and CrewAI each provide orchestration primitives but treat context as ephemeral. There is no standard way to persist, version, or audit the context an agent consumed. The paper identifies this as the critical gap: **no unified architectural foundation for traceability and governance**.

**The proposed solution.** An "Agentic File System" (AFS) that provides a uniform interface over heterogeneous context sources — conversations, documents, tool outputs, memory — with mandatory persistence, versioning, and access control.

## What dapple-prompt Does

We don't use the LLM-as-OS metaphor, but we share the underlying concern. Our architecture is **database-centric** rather than filesystem-centric:

**Instead of a virtual file system, we use PostgreSQL schemas.** The `rev` schema handles versioning, audit, and analytics. The `web` schema handles user-facing features. The `auth` schema handles NextAuth.js authentication. Schema boundaries provide the same separation that the paper achieves through filesystem mount points.

**Instead of file paths, we use content-addressable hashes.** Every versioned object in `rev.versioned_objects` carries a SHA-1 `content_hash`. Playbooks, stages, and steps use a Merkle pattern: parent hashes include sorted child hashes, so any descendant change propagates upward. This provides stronger integrity guarantees than filesystem metadata — you can verify that a context snapshot is identical to a previous one by comparing a single hash.

**Instead of embedding-based memory alone, we combine pgvector with structured relational data.** Guide content embeddings (`rev.guide_content_embeddings`) provide semantic search. But the embeddings point back to specific blocks, chapters, and guides via foreign keys. Tag and locale junction tables (`rev.guide_content_embedding_tags`, `rev.guide_content_embedding_locales`) enable structured filtering before vector similarity kicks in. This is richer than pure embedding-based memory (mem0) or pure graph-based memory (Zep).

**Instead of framework-level orchestration, we use a typed pipeline.** The guide ingest pipeline is `parse_guide() -> plan_guide() -> materialize_guide()`, where each stage has well-defined input and output types verified at compile time. The flow engine (`dapple-prompt-flow`) orchestrates dispatch execution with JSONLogic directives, Nushell scripting, and typed data namespaces (`data.*`, `metadata.*`, `temp_data.*`). This is not a "framework" in the LangChain sense — it's a compiled, type-checked system.

**Fluent localization as heterogeneous context source.** The paper discusses the challenge of heterogeneous context (different formats, languages, modalities). Our Fluent integration handles the multilingual dimension: all localizable text lives in `.ftl` files, parsed into `FluentMessageDef` records, grouped into `FluentMessageSet` collections, and materialized into `rev.fluent_messages`. The guide structure itself is locale-independent — a single `IngestPlan` produces objects that can be rendered in any locale.

## Comparison

**Alignment:** Both architectures recognize that the critical gap in current tooling is persistence and traceability. The paper's critique of LangChain/AutoGen as "fragmented" and "transient" matches our design motivation for building a database-centric system with append-only semantics and full version history.

**Divergence:** The paper proposes a filesystem abstraction as the unifying layer. We chose a relational database. This is not a superficial difference:

| Concern | File System Approach | Database Approach |
|---------|---------------------|-------------------|
| **Querying** | Path traversal, grep, custom indexing | SQL, joins, aggregates, pgvector similarity |
| **Transactions** | Application-level locking | ACID transactions, atomic multi-table inserts |
| **Versioning** | Git-like snapshots or custom | Content-addressable hashes on versioned_objects |
| **Access control** | POSIX permissions, ACLs | Row-level security, schema-level isolation |
| **Relationships** | Implicit (directory structure) | Explicit (foreign keys, junction tables) |
| **Search** | Full-text search add-ons | Built-in pgvector, tsvector, GIN indexes |

The relational approach trades the universality of "everything is a file" for the power of explicit relationships, type safety, and ACID guarantees. For a system that manages versioned content with complex relationships (guides -> chapters -> blocks -> tags, playbooks -> stages -> steps -> tasks), this is a significant advantage.

**What we can learn:** The paper's survey of memory systems (MemGPT's page-in/page-out, Graphiti's temporal knowledge graphs) suggests patterns we haven't explored. Our current architecture is flat — all context is equally accessible via SQL queries. A tiered approach where frequently-accessed context is cached differently from archival context could improve performance at scale.

## TLDR

The paper proposes an LLM-as-OS paradigm with a virtual file system as the unifying abstraction for context. dapple-prompt achieves the same goals — persistence, traceability, governance — through a database-centric architecture with content-addressable versioning and typed pipelines. The relational approach provides stronger guarantees for complex relationships and transactions, while the filesystem approach offers more universality. The paper's survey of memory hierarchies (MemGPT, Graphiti) points to tiered caching patterns worth exploring.
