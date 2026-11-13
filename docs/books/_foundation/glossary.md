# Glossary & Terms

> **Audience:** Prompt Engineers · Product · Agents · Developers

This glossary provides quick definitions for key terms used across the dapple-prompt platform.

---

<!-- ANCHOR: core-execution-model -->
## Core Execution Model

| Term | Definition |
|------|------------|
| **Task** | A single unit of work implemented as a small script. Tasks have clear inputs, outputs, and exit behavior. |
| **Setup Step** | A dynamic sub-interaction *within* a Stage (one round-trip of work + any operator input) before the user navigates on — the AJAXy part of a pane. Canonical shape: generate → pick → refine. Multi-step stages are the norm. A Step groups Tasks with guaranteed ordering and deterministic artifact passing. |
| **Stage** | A static *form / pane* the user lands on and advances *between*. Holds one or more Setup Steps (the interactions on that pane). Stage completion is the roll-up of its steps. |
| **Playbook** | A user-facing orchestration that combines Stages into a full flow. |
| **Playbook Instance / Run** | A single execution of a Playbook (sometimes called a run or session). The runtime creates one instance per user-triggered execution. |
| **Journey** | A product-level story or experience composed of one or more Playbooks, UX decisions, branching rules, and longer-lived state. A Journey is higher-level than a Playbook. |
<!-- ANCHOR_END: core-execution-model -->

<!-- ANCHOR: briefs-and-packs -->
## Briefs & Packs

| Term | Definition |
|------|------------|
| **Brief** | A single prompt-driven operation or small playbook-like fragment intended to be reused inside Playbooks (short for brief instruction). |
| **BriefGroup** | A logical grouping of related Briefs that must be completed atomically. |
| **Brief Pack** | A package of Playbooks, Briefs, prompts, example inputs, and metadata intended for distribution. |
| **Pack Manifest** | The manifest file (YAML/JSON) at the root of a Brief Pack describing playbooks, prompts, examples, required binaries, and metadata. |
<!-- ANCHOR_END: briefs-and-packs -->

<!-- ANCHOR: guides-and-content -->
## Guides & Structured Content

| Term | Definition |
|------|------------|
| **Guide** | The primary authoring unit in dapple-prompt. A versioned, structured content entity composed of chapters and blocks that derives backend objects (playbooks, tasks, inference profiles) through the ingest pipeline. Guides are the single source of truth — all platform objects originate from guide directories parsed and materialized via the API. |
| **GuideChapter** | An ordered section within a Guide, corresponding to a chapter in the source material. Chapters can be nested (parent-child) to model document hierarchy. Each chapter contains ordered blocks. |
| **GuideBlock** | A content unit within a GuideChapter. Each block has a directive type (e.g., `prose`, `stage`, `step`, `nushell`), optional name, raw content, key-value attributes, and audience tags. Blocks are the atomic content units for rendering and filtering. |
| **Directive Type** | A classification for guide blocks that describes the content's role. Common types: `prose` (narrative text), `stage` (stage definition), `step` (step definition), `nushell` (script), `brief_pack` (brief pack definition), `inference_profile` (model configuration), `ref` (cross-reference), `block` (generic). Stored in the `rev.guide_block_directive_types` lookup table. |
| **Guide Tag** | An audience-targeting label on guide blocks (e.g., `agent`, `product`, `coder`, `auditor`, `planner`). Tags enable filtering blocks by audience when rendering content. Stored in the `rev.guide_tags` lookup table. |
| **Shared Content** | *Deferred to V2+.* Reusable content modules in `_shared/` directories (scoped as `common` or `internal`) that guides could reference via `:::ref` blocks. The pipeline (handler, parser, DB tables) was removed during the path-layer refactor; shared content will be redesigned with proper `rev.shared_content_path_details` when it returns. |
<!-- ANCHOR_END: guides-and-content -->

<!-- ANCHOR: guide-engineering -->
## Guide Engineering Internals

| Term | Definition |
|------|------------|
| **ParsedGuide** | The intermediate AST produced by `dapple-parser` from a guide directory. Contains metadata, chapters with classified blocks, asset paths, and Fluent locale data. Pure data structure — no database access involved. |
| **IngestPlan** | The output of the transform layer (`plan_guide()`). Contains everything needed to materialize a guide: chapter/block definitions, derived objects (playbook, tasks, brief packs, inference profiles), Fluent message sets, asset file entries, and validation warnings. |
| **FluentMessageSet** | A collection of localized message bodies for a single Fluent key across all locales. During materialization, each set becomes a FluentMessage path + versioned object in the database. |
| **SharedContentPlan** | *Removed (V2+ deferral).* Was the transform output for shared content. The struct, the `plan_shared_content()` function, and `dapple-parser/src/shared.rs` were removed when the path-layer refactor dropped `object_paths.ref_key`. |
| **Materialization** | The process of persisting an `IngestPlan` to the database within a single transaction. Includes asset upload (draft/chunk/finalize), Fluent message creation, guide insert via atomic CTE, and derived object creation with slug+version dedup. |
| **dapple-parser** | The Rust crate that parses guide directories into `ParsedGuide` ASTs and transforms them into `IngestPlan`s. Contains the fence tokenizer, block classifier, attribute parser, Fluent discovery, and SUMMARY.md parser. |
| **dapple-render** | The Rust crate that generates filtered PDFs from guide content. Applies render profiles (tag filters + branding styles) to produce audience-specific document variants. |
| **dapple-sdk** | The Rust SDK crate providing a typed HTTP client for the dapple-prompt API. Types are auto-generated from the OpenAPI spec via typify. Supports API token, JWT, and Ed25519 service token authentication. |
| **DappleClient** | The main SDK client struct. Provides accessor methods (e.g., `.guides()`, `.playbooks()`) that return typed resource clients for each API endpoint. |
| **Resource Client** | A typed sub-client in the SDK (e.g., `GuidesClient`, `PlaybooksClient`) that provides ergonomic methods (`get`, `list`, `create`, `ingest_from_dir`) for a specific API resource. |
| **Render Profile** | A configuration that defines which blocks to include (via tag filter) and how to style the output (branding, watermark, cover page). Stored in `rev.render_profiles`. |
<!-- ANCHOR_END: guide-engineering -->

<!-- ANCHOR: surfaces-and-tenancy -->
## API Surfaces & Tenancy

| Term | Definition |
|------|------------|
| **Internal Surface** | The `dapple-prompt-api` REST API served on `api.dapple.x`. Used by dapple-team services (dapple-ui, dapple-conduit, dapple-flow). Single-tenant — does not filter queries by company. |
| **External Surface** | The `dapple-prompt-external-api` REST API served on `external-api.dapple.x`. Used by customer companies (third parties). Has its own service-token table (`rev.external_service_tokens`), its own JWT signing key (`DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY`), and its own extractor (`ExternalCaller`). Every query filters by the caller's `company_id`. |
| **Surface Isolation** | The cryptographic + structural guarantee that a JWT minted for one surface cannot be used on another. Verification fails at signature check before any claim is read; the keys are different. |
| **`rev.companies`** | The canonical org primitive. Every namespace-bearing row in `rev.object_paths` and `rev.canonical_release_pins` references a company via `namespace_company_id UUID FK`. The dapple system org has a fixed UUID (`019e132e-2610-7a4a-b64a-77a49e286ac0`) seeded by Migration 31, also exported as `DAPPLE_COMPANY_UUID` in the nushell environment. |
| **Company Slug** | Immutable URL-safe identifier for a `rev.companies` row (regex `^[a-z][a-z0-9_-]{1,31}$`). Used in JWT payloads, audit logs, and admin tooling. Mutability is rejected by a BEFORE UPDATE trigger. |
| **Scope Tier** | The flat enum on `rev.object_paths.scope` and `rev.canonical_release_pins.scope`: `public`/`common`/`internal` (dapple namespace only) and `external` (non-dapple namespaces only). `external` mirrors `internal` for customer-private content. |
| **`ExternalCaller`** | The Axum extractor on the external surface. Decodes the Bearer JWT with the external signing key, validates `iss`/`token_use`/`exp`, and returns `(company_id, company_slug, service_id)`. Every external handler takes this as a parameter; no code path bypasses it. |
| **External Service Token** | Customer-owned Ed25519 service token registered in `rev.external_service_tokens`. The customer signs a 128-byte token with their private key, exchanges it at `POST external-api.dapple.x/api/get-service-token`, and receives a JWT bound to their company. |
<!-- ANCHOR_END: surfaces-and-tenancy -->

<!-- ANCHOR: artifacts-and-files -->
## Artifacts & Files

| Term | Definition |
|------|------------|
| **Artifact** | Any file or structured output produced by a Task (e.g., a Markdown resume, a PDF, a JSON metadata file). |
| **artifact.json** | The minimal contract file a Task should write when producing a primary artifact. Schema: `{ "path": "./out/resume.pdf", "type": "application/pdf" }`. |
| **checkpoint.json** | Optional file written at step or stage boundaries to record human-review decisions or approvals. |
| **log.jsonl** | Newline-delimited JSON structured logs that Tasks write to `OUT_DIR` for machine consumption and runtime tracing. |
| **prompt-debug.json** | An optional debug artifact that records the substituted prompt and raw model response for troubleshooting LLM calls. |
| **fluent-trace.json** | A trace of Fluent Messages (system/user/assistant/tool) exchanged during an LLM interaction. Useful for auditing and replay. |
| **pending.json** | A small file written by a Task to indicate an async request is outstanding, containing a `request_id` and other metadata. |
| **error.json** | An optional file that a failing Task can write with `{ "message": "...", "code": 123 }` to explain non-zero exits. |
<!-- ANCHOR_END: artifacts-and-files -->

<!-- ANCHOR: environment-variables -->
## Environment Variables

| Term | Definition |
|------|------------|
| **IN_ARTIFACT** | Environment variable set by the runtime for the next Task that points to the previous Task's primary artifact path. |
| **OUT_DIR** | Environment variable where the Task should write artifacts, logs, and debug traces. |
| **Playbook ID / Stage ID / Step ID / Task ID** | Stable identifiers used to label parts of a Playbook and exposed via environment variables for observability. |
<!-- ANCHOR_END: environment-variables -->

<!-- ANCHOR: prompts-and-llm -->
## Prompts & LLM Integration

| Term | Definition |
|------|------------|
| **Prompt** | A text template (often with placeholders) sent to an LLM to drive extraction, normalization, or content generation. |
| **Prompt Template** | A reusable prompt file that contains placeholders like `{{user_text}}` or `{{input_json}}` intended for substitution at runtime. |
| **Prompt ID** | A stable identifier (e.g., `resume-data-extractor.v1`) used to reference a prompt template in manifests, Tasks, or overlays. |
| **Prompt Registry** | (WIP) A platform-level service that stores versioned prompts and returns a Prompt ID. |
| **Prompt Harness** | A small, author-side helper script that substitutes inputs into prompt templates for manual testing. |
| **Fluent Message** | A structured message model used when interacting with LLMs; messages include types (system/user/assistant/tool) and optional JSON payloads. |
<!-- ANCHOR_END: prompts-and-llm -->

<!-- ANCHOR: conduit-and-routing -->
## Conduit & Routing

| Term | Definition |
|------|------------|
| **Conduit (dapple-prompt-conduit)** | The runtime bridge that routes prompt templates and substituted prompts to a configured LLM provider. Handles provider selection, tracing, and retries. |
| **CONDUIT_CALL / CONDUIT_OPTIONS** | Author-facing runtime hooks (environment variables or wrapper CLI) through which Tasks call the conduit. `CONDUIT_OPTIONS` is a JSON blob that describes provider/model/options. |
| **Overlay** | A runtime configuration bundle that overrides prompt routing/conduit behavior without editing the prompt. Overlays can be attached at prompt, playbook, pack, or runtime scope. |
| **overlay_id** | The identifier for an applied overlay; authors should include this id in debug traces to correlate behavior and billing. |
<!-- ANCHOR_END: conduit-and-routing -->

<!-- ANCHOR: model-taxonomy -->
## Model Taxonomy

| Term | Definition |
|------|------------|
| **Provider** | Where a model is hosted and billed (e.g., Anthropic API, Self-Hosted, NVIDIA NIM). The same lineage can exist at multiple providers. |
| **Lineage** | The prompting paradigm - how a model family responds to prompts. Models in the same lineage understand prompts structured the same way. Examples: `claude-3-5`, `vits`, `flan-t5`. |
| **Tier** | The capability level within a lineage. Examples: `sonnet`/`opus`/`haiku` for Claude, `ljspeech`/`vctk` for VITS. |
| **Release** | A specific version snapshot from the provider. Examples: `20241022`, `v0.1`. |
| **Variant** | A fine-tuned model that branches from a specific Release. Used for custom adaptations or A/B testing. |
| **Release Chain** | The fallback resolution order for template matching: `@provider:lineage/tier:release` → `lineage/tier` → `lineage`. |
<!-- ANCHOR_END: model-taxonomy -->

<!-- ANCHOR: inference-configuration -->
## Inference Configuration

| Term | Definition |
|------|------------|
| **InferenceProfile** | A versioned entity that maps prompts to model options. Assigned to Journeys at creation time, enabling Playbooks to be model-agnostic. |
| **InferenceProfileConfig** | A single mapping from a prompt path to a model (release or variant) with a priority. Multiple configs per prompt allow fallback selection. |
| **ModelService** | The Conduit service that resolves model configuration from InferenceProfiles and selects the best available model at runtime. |
| **ModelConfig** | The resolved configuration for a prompt execution, containing lineage, provider, tier, release, and sampling parameters. |
<!-- ANCHOR_END: inference-configuration -->

<!-- ANCHOR: best-practices -->
## Best Practices

| Term | Definition |
|------|------------|
| **Idempotency** | A property of a Task that allows safe retries without unintended side effects. Design Tasks to be idempotent or write clear resume markers. |
| **Versioning** | A naming pattern for prompts, playbooks, and packs. Use dotted suffixes for prompts and semantic versions for packs (e.g., `resume-data-extractor.v1`, `0.1.0`). |
| **Naming conventions** | Use short, kebab-case ids for Tasks/Steps/Stages/Playbooks (e.g., `format-resume`, `build-resume-step`, `stage-1`, `build-resume-playbook`). |
| **Human Review / Debrief** | A UX pattern where a Playbook pauses to request human approval or presents a debrief summary after completion. |
<!-- ANCHOR_END: best-practices -->

<!-- ANCHOR: testing -->
## Testing

| Term | Definition |
|------|------------|
| **ScenarioContext** | A testing helper used in integration tests to store named batches and created objects. Not required for authoring Playbooks. |
<!-- ANCHOR_END: testing -->

<!-- ANCHOR: authentication -->
## Authentication & API Access

| Term | Definition |
|------|------------|
| **Opaque Personal Access Token (PAT)** | A user-scoped, hash-stored token for programmatic API access. Unlike JWTs, opaque tokens are random strings that require database lookup for validation. Prefixed with `dpl_` for identification. Similar to GitHub PATs (`ghp_`) or GitLab PATs (`glpat-`). |
| **JWT (JSON Web Token)** | A self-contained token that encodes claims (user ID, roles, expiration) and can be validated without database lookup. Used for web sessions via NextAuth.js. |
| **Ethereum-Signed Auth Token** | A short-lived token signed with an Ethereum private key, exchanged for a JWT. Used for trusted external services with wallet infrastructure. |
| **Token Prefix** | The `dpl_` prefix on opaque PATs that enables immediate token type detection without parsing. Agents detect `dpl_` → hash lookup, no prefix → JWT validation. |
| **Token Hash** | The SHA-256 hash of an opaque PAT stored in `web.api_tokens.token_hash`. The plaintext token is never stored; validation computes the hash and performs constant-time comparison. |
<!-- ANCHOR_END: authentication -->

---

*If a term you expect is missing, please request it and we'll add a concise definition.*
