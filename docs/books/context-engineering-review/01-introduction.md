# Introduction — Context Engineering as Architectural Concern

## What the Paper Says

The paper opens by reframing what practitioners call "prompt engineering" as a much broader discipline: **context engineering**. The argument is that crafting a single prompt is the easy part. The hard part is the full lifecycle around that prompt:

1. **Selection** — deciding which information is relevant
2. **Retrieval** — pulling it from heterogeneous sources
3. **Filtering** — removing noise and enforcing governance
4. **Construction** — assembling the context window
5. **Compression** — fitting within token limits
6. **Evaluation** — assessing output quality
7. **Refresh** — updating stale context

The paper's core critique is that current tools — LangChain, AutoGen, CrewAI — treat these steps as ad-hoc plumbing. Context artifacts are transient (in-memory, discarded after a session). There is no traceability: you cannot reconstruct what context an agent saw when it produced a given output. This makes debugging, auditing, and governance impossible at scale.

The paper positions context engineering as an **architectural concern** that deserves the same rigor as database schema design or API versioning.

## What dapple-prompt Does

The guide ingest pipeline is dapple-prompt's answer to this lifecycle, and it maps cleanly onto the paper's seven stages:

| Paper Stage | dapple-prompt Implementation |
|-------------|------------------------------|
| **Selection** | `parse_guide()` discovers content from a guide directory: markdown chapters, Fluent `.ftl` files, TOML metadata, asset files. `KNOWN_DIRECTIVES` filters to recognized block types. |
| **Retrieval** | Guide tarballs are uploaded via multipart POST. `extract_tarball_field()` decompresses, `find_guide_root()` locates `guide.toml`, and the parser walks the directory tree. |
| **Filtering** | `plan_guide()` is a pure transform that groups Fluent messages by key, extracts brief packs, builds the playbook hierarchy, and collects assets — discarding anything unrecognized. Tag-based block filtering at render time (`dapple-render`) further curates output. |
| **Construction** | `materialize_guide()` atomically inserts the full object graph into PostgreSQL within a single transaction: guide, chapters, blocks, tags, assets, cards, derived playbooks, tasks, inference profiles. |
| **Compression** | RAG retrieval (`RagService::retrieve()`) selects the most relevant guide blocks via pgvector similarity and limits results. InferenceProfile configs carry `max_tokens` to bound output. Fluent bodies resolve per-locale, avoiding redundant multilingual content in the context window. |
| **Evaluation** | The directive system (`DirectiveEvaluator`) evaluates step outputs via JSONLogic, triggering retry, branch, skip, or proceed. Human-in-the-loop evaluation happens through debrief responses. |
| **Refresh** | Content-addressable versioning: PATCH operations compute a new `content_hash` and short-circuit if it matches the latest version. Guide re-ingest creates a new version only when content has actually changed. Embeddings are regenerated asynchronously after ingest. |

Nothing in this pipeline is transient. Every artifact — from the parsed guide structure to the materialized database objects to the generated embeddings — persists in PostgreSQL with full version history. The `rev.versioned_objects` table, `rev.task_attempts` audit trail, and dispatch events provide the traceability the paper identifies as missing from current tools.

## Comparison

**Alignment:** The paper's central thesis — that context engineering is an architectural concern, not a scripting exercise — is exactly the design philosophy dapple-prompt embodies. We invested in a relational data model, compile-time SQL verification, content-addressable hashing, and atomic transactions precisely because we treat context as infrastructure, not glue code.

**Divergence:** The paper frames this as a new insight. For dapple-prompt, it was a founding assumption. The system was designed from the start around persistent, versioned, auditable context — not retrofitted from an ad-hoc prototype. This means we never had the "transient artifact" problem the paper diagnoses.

**What we can learn:** The paper's explicit enumeration of the seven lifecycle stages is a useful vocabulary. We perform all seven, but we don't name them as such in our documentation or code comments. Adopting this taxonomy could improve onboarding and architectural discussions.

## TLDR

The paper argues that context engineering — the full lifecycle of selecting, retrieving, filtering, constructing, compressing, evaluating, and refreshing context — is an architectural concern, not a prompting trick. dapple-prompt's guide ingest pipeline already implements all seven stages with persistent, versioned, auditable artifacts. The paper's contribution to us is primarily vocabulary: naming what we already do.
