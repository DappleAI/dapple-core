# dapple-shell microservice + dataset pipeline — musing

> **Status:** exploratory. This is not a commitment, it's a thinking document. The contents below were drafted during a planning session for `brief-flow-improvements.md` and broke out when a question about the polars integration approach surfaced an architectural decision (shell-as-microservice) that is larger than the immediate work. We deliberately set it aside to finish the brief-flow plan first. The next session on this topic will be an interview driven off the questions at the bottom of this file.
>
> **Author:** drafted collaboratively with Claude during plan mode, 2026-04-11.
>
> **Supersedes:** nothing. Partially overlaps with the asset upload work originally planned inside brief-flow-improvements, which assumed embedded polars in `dapple-prompt-flow`. If the shell refactor is adopted, that work would be replanned.

## What triggered this document

While planning how tabular-data slots (`value-slot { kind: "dataset" }`) would work for the brief system, we hit a sequence of realizations:

1. The canonical extraction format for tabular data should be **parquet**, for round-tripping via polars.
2. Polars is not currently in the `dapple-prompt` workspace — adding it adds a heavy dependency (~30s compile time, significant binary-size hit) to whichever crate receives it.
3. The natural home for polars is `dapple-prompt-flow` because nushell task scripts would need it — but flow is already large and this adds more.
4. The familiar nushell ergonomic for tabular data is `nu_plugin_polars` (the `dfr` command family), which runs as an external plugin process. We already have latent concerns about IPC overhead from earlier work on other plugins.
5. The project's own plugin docs (`crates/dapple-prompt-flow/src/engine/plugins.rs`) explicitly recommend **built-in commands over plugins** for internal use, citing IPC cost. Every existing custom command (`pdf`, `prompt`, `summarize`, `synth`, `madlib`, `json_schema`, `guide_search`) is a built-in registered via `working_set.add_decl`. No plugins are wired today.
6. That means "use polars via dfr" fights the project convention, AND "use polars as a built-in" bloats flow.
7. The user then surfaced a latent interest: **"maybe a `dapple-shell` service would be worthwhile anyway even without worrying about IPC"** — separating the shell execution layer into its own microservice, with overlays running thinner per-task for capability-based security.

That last thought is the entire reason this document exists. The immediate polars decision is small; the shell-as-microservice question is large, and getting it wrong in either direction has compounding costs. This musing is the space to think through it before committing.

## Current architecture — what we'd be departing from

### How nushell tasks execute today

`dapple-prompt-flow::engine::StepEngine` embeds nushell directly via the crate family:

- `nu-engine` — core execution
- `nu-parser` — script parsing
- `nu-protocol` — types, signatures, pipeline data
- `nu-command` — standard library of nushell commands
- `nu-cmd-lang` — language constructs

The `StepEngine::new()` call builds an `EngineState`, registers nu-command's standard library, registers our custom commands, applies the security policy's allowlist, and caches the result. A new StepEngine is (or should be) constructed per worker/step, and each one caches parsed scripts by their SHA hash so repeated execution of the same task is cheap.

### Custom commands

Located in `crates/dapple-prompt-flow/src/commands/`:

- `prompt.rs`, `prompt_send.rs`, `prompt_retry.rs`, `prompt_reset.rs` — LLM interaction via conduit
- `pdf.rs`, `pdf_fetch.rs`, `pdf_extract.rs` — PDF handling (the pattern we already identified as broken in the brief-flow plan)
- `summarize.rs`, `synth.rs` — flan-t5 summarization, vits TTS
- `madlib.rs` — MLM fill-mask via ModernBERT
- `json_schema.rs` — JSON Schema inference
- `guide.rs`, `guide_search.rs` — guide lookup and pgvector similarity search

Each is a `impl Command` with a name, signature, and `run` method. Registration happens in `commands/mod.rs` or the StepEngine constructor via `working_set.add_decl(Box::new(MyCommand))`. No IPC, no plugin protocol — they're just Rust functions that nushell calls directly.

### Security policy

`SecurityPolicy::validate_script` runs at **parse time** — the nushell AST is walked and the policy rejects any call to:

- External commands (`^ls`, `^curl`, etc.)
- Environment mutations (`$env.FOO = ...`)
- Plugin management (`plugin add`, `plugin rm`)
- Anything not on the allowlist, if running in allowlist mode

The policy is parse-time only — there's no runtime sandboxing. Once a script passes validation, it runs with whatever access the custom commands give it. If `pdf fetch` can hit arbitrary HTTP, a validated task script can invoke it to hit arbitrary HTTP. If `guide search` has DB access, a validated task script can query the DB.

### Blast radius today

Every task runs in the same process as `dispatch_runner`. A compromised task has:

- Full access to all registered custom commands (not scoped to the task's declared needs)
- Full memory address space of the flow service
- Full access to anything flow has access to (DB connections via `data.db`, conduit HTTP client, asset store via the chunk query layer)
- No per-task resource limits (CPU, memory, wall-clock)

Parse-time allowlisting is a real mitigation but it's load-bearing on the assumption that *every* command in the allowlist is safe when invoked with *any* input. That assumption gets weaker as the command set grows and as the commands themselves grow more capable (e.g., `guide search` takes arbitrary SQL-ish filters, `prompt send` reaches external LLMs with arbitrary prompt text).

## The proposal — `dapple-shell` as a microservice

Instead of embedding nushell in flow, split it into its own service. dispatch_runner becomes a client; dapple-shell becomes the execution engine.

### Sketch

```
┌─────────────────────┐     script + input + caps      ┌──────────────────────┐
│  dapple-prompt-flow │ ─────────────────────────────► │    dapple-shell       │
│                     │                                │                      │
│  dispatch_runner    │ ◄───────────────────────────── │  nushell execution    │
│                     │     payload + signals          │  custom commands      │
│  (orchestration)    │                                │  overlay-scoped caps  │
└─────────────────────┘                                └──────────────────────┘
```

Task execution flow:

1. dispatch_runner picks a task from the dispatch tree.
2. It parses the task's script AST locally (or delegates even that) to determine the overlay capability set ("this task calls `prompt send` and `pdf extract`, so it needs the `prompt` and `pdf` overlays").
3. It sends the script, the input payload (or references to the input), and the capability set over the wire to dapple-shell.
4. dapple-shell spawns whatever isolation unit it uses per task (fork, warm worker, WASM instance — TBD), loads only the declared overlays, executes the script, and returns the payload + signals.
5. dispatch_runner records the result in the dispatch context and moves on.

### Why split at all?

Three intertwined motivations:

**(a) Security** — the ability to isolate task execution from the orchestration layer and from each other. Process-per-task (or equivalent) blocks lateral memory-sharing attacks, bounds resource usage, and creates a clean trust boundary.

**(b) Capability scoping via overlays** — nushell's overlay system becomes a capability mechanism. A task declares which overlays it needs; the shell service validates the request against a whitelist and loads only those overlays. A task that doesn't declare `pdf` literally cannot call pdf commands even if the script contains a `pdf` reference — parse-time rejection in dapple-shell would flag it.

**(c) Operational cleanness** — custom commands become independently versionable. A pdf crate bump requires rebuilding shell, not flow. polars (or any other heavy dep) lives in shell, not bloating flow. Shell can scale independently. Flow is smaller and easier to reason about as pure orchestration.

The user has also floated (implicitly) that this direction is coming eventually, and the trigger for this document is the realization that polars-in-flow might be throwaway work if shell is going to happen.

## Architectural dimensions — all of them, explicitly

### Dimension 1: Trust boundary and isolation model

This is the pivotal question because **"microservice" alone doesn't buy you security**. A long-lived dapple-shell process running every task in the same OS address space has the same blast radius as the current embedded model — compromised task A can still clobber task B's memory if they run in the same process. The security win comes from process-level isolation per task, which means picking one of:

- **Fork-per-task (Unix process per script)** — cheap on Linux (~1-5ms fork + exec), strong memory isolation, per-process OS resource limits (ulimits, cgroups), dies on completion. Natural fit for short-lived nushell scripts. Downsides: warm caches (parsed scripts, loaded shared libs) are lost on each fork unless we use `fork()` without `exec()` to inherit from a pre-initialized parent. Engine state initialization takes non-trivial time for nushell — we'd want the parent process to hold a pre-built EngineState and each fork to start with a copy-on-write clone.

- **Warm worker pool with per-request reset** — N long-lived workers, each handles one request at a time and explicitly resets nushell EngineState between requests. Lower per-task latency (no fork overhead). Security is weaker — reset correctness is load-bearing, and bugs in the reset (e.g., not clearing a global, a custom command holding mutable state) leak across tasks. Requires careful audit.

- **WASM sandbox per task** — compile nushell and all custom commands to WASI. Each task runs in a fresh Wasmtime instance. Strongest memory isolation and resource limits (Wasmtime has fuel metering, memory caps, syscall filtering). Cost: nushell-in-WASM is untested territory, the port effort is significant, and any custom command that reaches native code (HTTP, DB drivers) doesn't work in WASI without proxying via host functions. Closest to "actually secure" but highest lift.

- **Container-per-task (firecracker, gvisor)** — overkill for millisecond tasks, reasonable for minute-scale ones. Strongest isolation (VM boundary), highest overhead. Probably only relevant for tasks that run untrusted user-provided scripts in a multi-tenant scenario, which isn't the current use case.

**Doubts I have:**

- Fork-per-task sounds right but I don't actually know the nushell EngineState construction cost. If it's 200ms to initialize, fork-without-exec works fine (copy-on-write is fast). If it's 5s, we need a pre-warmed parent.
- Warm worker pool is tempting because of simplicity, but "reset the engine state correctly" is the kind of invariant that breaks under pressure and we won't notice until something leaks.
- WASM is "the right answer" in a principled sense but might be a year of work. Worth exploring but definitely not the near-term move.
- We haven't benchmarked anything. Any choice here should be backed by measurements, not vibes.

### Dimension 2: Overlay-as-capability mechanism

Overlays in nushell today are organizational. A script can `overlay use foo` at any point unless something prevents it. For overlays to *be* capabilities, you need all three of:

1. **A service-level whitelist**: dapple-shell knows which overlays exist and which overlays each incoming request is allowed to use. The request carries a capability claim ("I'm allowed to use overlays X, Y, Z") that shell validates before execution.

2. **Parse-time enforcement**: shell walks the AST of every incoming script and rejects any `overlay use` statement that names an overlay outside the declared set. Extends the existing SecurityPolicy pattern. Must also catch indirect overlay loading — if nushell has a way to load an overlay dynamically from a variable, that's a hole.

3. **Signed / verified capability declarations**: how does shell know the capability set is legitimate? Options:
    - dispatch_runner is the only caller and it's trusted — shell trusts any caller inside its security perimeter
    - Capability sets are signed with a dispatch-runner key; shell verifies the signature
    - Shell derives the capability set itself from the script (AST walk → command names → overlay map), ignoring whatever the caller sent. Most robust because there's no trust on the declaring layer.

**Who declares the capability set in the first place?** Three options, escalating in automation:

- **(a) Task author declares manually** — nushell blocks carry an `overlays: ["prompt", "pdf"]` attribute. The guide builder or human author writes it. Error-prone but transparent.
- **(b) Guide builder LLM infers it** — during `build-chapter-content-task` or similar, the builder looks at the script's commands and emits the overlay list. Downstream of LLM reliability concerns.
- **(c) Auto-derived at dispatch time** — dispatch_runner walks the parsed AST of every task script, collects command names, maps to overlays via a static table, and sends the derived capability set. No human or LLM in the loop for the derivation itself. The mapping table (`pdf extract → pdf overlay`, `prompt send → prompt overlay`, etc.) becomes the audit surface.

I lean heavily toward (c). It's the only option where the capability declaration is a *mechanical consequence* of the script's contents, not a separate claim that could drift from what the script actually does. The cost is maintaining the command-to-overlay mapping table, which is small and reviewable.

**Doubts:**

- Does nushell allow dynamic overlay loading? `overlay use $some_var`? If yes, the AST walk can't fully enumerate needed overlays without evaluating. We'd have to either disallow dynamic overlay loading entirely or treat any dynamic load as requiring the union of all allowed overlays (defeats the purpose).
- What counts as an "overlay"? nushell overlays can be files or modules. In our world they'd be logical groupings of custom commands. We'd need to define the overlay taxonomy upfront: `pdf`, `prompt`, `summarize`, `dataset`, `guide_search`, etc.
- Overlays are currently compile-time in our sense — the commands are all registered at StepEngine construction. Switching to overlay-gated loading means we need to register commands *lazily* per request, which is a structural change to the nushell bootstrap.

### Dimension 3: Transport and latency budget

The current embedded model is zero-latency (direct function calls). Adding a shell service adds a round-trip per task execution. The cost matters more for some task profiles than others.

Options:

- **Unix socket + msgpack**: sub-millisecond on localhost. Requires colocated deployment (same pod / same node). Fine for a local sidecar.
- **Loopback HTTP/1.1**: 1-5ms per call. Easier to reason about, reusable tooling, debuggable with curl. Viable when total task time is > 50ms.
- **gRPC over loopback**: similar to HTTP in latency. Adds bidirectional streaming (useful for large payloads or progressive output), adds the gRPC dependency surface (tonic, prost). Good if we want the shell service to stream task output back in chunks rather than batching.
- **Cross-node (k8s service endpoint)**: 5-30ms per round trip. Acceptable for LLM-heavy tasks where the LLM call dominates. Painful if tasks do lots of small nushell operations.
- **Embedded IPC via shared memory / zerocopy** (e.g., tokio-util's codec over a ring buffer): sub-microsecond. Overkill for our volume.

**Task profile affects the choice.** If most tasks look like `prompt send $input | from json | select topic | to json` — one LLM call dominating wall-clock — IPC is free. If tasks look like "open a dataset, filter, select, summarize, pipe to prompt send" — lots of small operations — IPC compounds.

**Doubts:**

- I don't actually know the task profile distribution. Would need to measure or estimate from existing guides.
- Streaming vs batching: is any task expected to produce output incrementally (e.g., streaming LLM output)? Today I think no — tasks return whole payloads. If that changes, streaming matters.
- Cross-node transport is a question if we want to scale shell instances independently from flow. Probably not worth it unless shell becomes CPU-bound.

### Dimension 4: Input and output payload transfer

Task inputs can be big. Examples:

- Prior task output with tens of KB of LLM text — fine for any transport.
- Full PDF extraction result (100s of KB, sometimes MB) — fine over localhost, painful over network.
- A polars DataFrame with millions of rows — catastrophic over the wire in any form.
- A bound asset (the asset itself, referenced by UUID, but the bytes could be huge) — can't serialize at all.

Options:

- **Pass everything by value** — every request carries the full input payload. Simple, bounded by practical size limits. Breaks on large inputs.

- **Shared DB / asset store** — dapple-shell has its own read access to Postgres (or a restricted subset) and to the chunk store. dispatch_runner passes *references* (asset_ids, dispatch_ids, binding slot names) and shell resolves them via its own DB queries. No big payloads over the wire. Cost: shell needs DB credentials, which expands its trust boundary. Pro: shell can retrieve exactly what it needs, when it needs it, without flow pre-serializing.

- **Content-addressed blobstore** — inputs over a size threshold get written to S3/local disk with a content hash; shell fetches by hash. Generic, doesn't require shell DB access. Adds a blobstore dependency we don't currently have.

- **Hybrid** — small inputs by value, large inputs by reference (threshold-based). Most pragmatic.

**Leaning toward shared DB access for shell.** It matches what the existing custom commands already need (`pdf extract` reads from the asset chunks, `guide search` queries pgvector, etc.). If shell has them, it needs the underlying DB access anyway. The security boundary becomes: shell has read access to `rev.assets`, `rev.chunks`, `rev.dispatch_intent_bindings`, pgvector tables, etc. — the data layer. Shell does NOT have write access to anything except maybe task_attempt audit tables. That tiered credential set is straightforward to arrange with Postgres roles.

**Doubts:**

- Does `prompt send` currently do anything that *requires* seeing the full input payload in process? E.g., if the task's input is a giant text blob and prompt send embeds it in the LLM prompt, shell needs the full text. Shared DB access doesn't help here — the payload is genuinely in motion.
- What about task outputs? If a task writes a 10MB JSON payload to `context_data`, that serialized output has to come back to dispatch_runner to be persisted. Return trip is unavoidable unless shell writes directly to the dispatch task attempts table itself (which crosses the "shell should not write data-layer state" line I just drew).
- How does this interact with the journey-wide binding seeding we did in the current plan? The dispatch_runner seeds `$in.bindings.*` from `select_bindings_by_journey_of_dispatch`. If shell has DB access, shell could do the seed itself. Cleaner responsibility split.

### Dimension 5: Where do custom commands live?

Currently all custom commands are in `dapple-prompt-flow::commands/`. In a shell microservice world, they migrate. But not uniformly — commands fall into capability tiers, and where they live should reflect their privilege level.

**Tier A — pure / referentially transparent:**
- `madlib` (ModernBERT MLM fill) — CPU work, no I/O
- `json_schema` — pure computation
- `summarize` (flan-t5) — CPU work via ONNX, no I/O

These are trivially safe to put in shell. No privileged access needed. Can run in any isolation unit without concern.

**Tier B — DB-reading:**
- `guide_search` — pgvector similarity query
- `pdf extract` (in the refactored model) — reads from asset_extractions table
- A future `dataset open` — reads from dataset_tables / asset chunks

These need DB read access. If shell has that credential, tier B lives in shell with a "db-read" overlay gate.

**Tier C — network-outbound / privileged:**
- `prompt send` — calls conduit over HTTP. Conduit in turn calls external LLMs. This is the primary exfiltration channel: a task with `prompt send` can send arbitrary text to an external provider.
- `pdf fetch` — arbitrary outbound HTTP GET. Hard to constrain safely. Probably the most dangerous command in the existing set.
- `synth` — calls conduit for TTS, which can be heavy but is bounded by conduit's own API.

Tier C is where I'd consider *not* putting them in shell directly, even with isolation. Options for Tier C:

- **Stay in a privileged sidecar (dapple-conduit-proxy or similar)** — shell makes an outbound request to a narrow proxy that only exposes the specific allowed destinations. Shell itself has no arbitrary outbound network. Tasks that need `prompt send` get an overlay that routes through the proxy. This gives you per-task logging of every outbound LLM call, and a single place to rate-limit / audit / filter.
- **In shell, but shell itself has no outbound HTTP** — the `prompt send` command inside shell connects to the proxy rather than directly to conduit. Same effect as above but packages the split slightly differently.
- **Accept the risk and put them in shell directly** — simplest, least secure. Documented and understood risk.

I would strongly prefer the proxy pattern for Tier C. It's not that much more work and it gives you a clean audit/policy point.

**Doubts:**

- `pdf fetch` specifically is a weird beast. It exists because guide ingest needed to fetch external PDFs at authoring time. Do runtime tasks (not just guide ingest) actually use `pdf fetch`? If it's only used at ingest time and never at dispatch time, it can stay in flow (or even move out of the nushell command set entirely) and not pollute shell's outbound story.
- `prompt send` has a very large surface (the prompt template system, the model config resolution, the RAG injection, etc.). Teasing it apart into "the bit that runs in shell" and "the bit that runs in a proxy" is non-trivial.
- Where do the conduit RAG retrieval results come from? I believe they're retrieved inside the conduit service itself, so shell's `prompt send` just hands off to conduit and conduit does the retrieval. If that's right, shell's interface is narrower than I feared.

### Dimension 6: Deployment shape

- **Local sidecar** — one dapple-shell instance per flow instance, colocated in the same pod (or running as a subprocess of flow). Unix socket transport. Simplest. Security model is "trust the whole pod".
- **Shared service** — a few dapple-shell replicas behind a service endpoint, shared by all flow instances. Horizontally scalable, but per-request routing and isolation decisions get harder. Probably overkill unless task volume is high.
- **Per-tenant instances** — strongest isolation for multi-tenant scenarios, most operational overhead. Only makes sense when tenants are mutually distrustful. Not the current scenario.

For single-tenant dev and sandbox, local sidecar is the natural starting point and can be scaled up later if needed.

**Doubts:**

- Local sidecar means flow must be able to spawn and manage the shell subprocess, OR shell must be started independently and discovered by flow. Process supervision is a mild complication.
- If shell is a subprocess of flow, its crash blast radius includes flow (systemd-like supervision helps but isn't free). If shell is an independent service, crash recovery is clean but discovery is needed.
- k8s makes the shared-service model easier than local sidecar in some ways (every deployment gets an automatic service endpoint) and harder in others (pod affinity to keep shell colocated with flow for latency, startup ordering, etc.).

### Dimension 7: Interaction with conduit

Conduit is the LLM service (`dapple-prompt-conduit`). It holds the model backends, the embedding service, the RAG service, etc. Today, task scripts call `prompt send` which calls conduit over HTTP.

In the shell microservice model, who talks to conduit?

- **Shell talks to conduit directly** — shell has the conduit HTTP client. Task scripts in shell call `prompt send`, which calls conduit. Flow doesn't know about conduit at this layer. Cleaner separation of concerns.
- **Shell asks flow to call conduit** — shell sends a "need LLM response" message back through its open connection to flow; flow calls conduit and returns the result. Keeps conduit access concentrated in flow but adds a round-trip.
- **Shell talks to a conduit proxy** — shell talks to a narrow proxy that fronts conduit and adds policy (rate limiting, logging, destination filtering). Flow is separate from the LLM access path entirely.

Option 3 is probably the safest; it keeps the proxy pattern from Tier C in dimension 5 consistent. Option 1 is simplest but puts more trust in shell.

Also: the RAG retrieval logic currently lives in conduit — when `prompt send` is called, conduit fetches relevant guide content from pgvector and injects it into the prompt. If shell is calling conduit, this doesn't change. If shell starts doing RAG retrieval itself (because shell has DB access), we'd be duplicating the retrieval pipeline. Don't do that.

### Dimension 8: Script caching

The current StepEngine caches parsed scripts by content hash so repeated execution doesn't re-parse. Where does the cache live in the new world?

- **In shell** — shell caches parsed scripts it has seen before, keyed by content hash. Parse work is amortized across all tasks that use the same script. Cache lives in shell's memory or a small local disk cache.
- **In flow** — flow sends an already-parsed intermediate representation to shell. More complex wire format, but offloads parse work to whichever process has better CPU headroom.
- **In both** — redundant but safe.

Shell-side is more natural — parsing happens where execution happens.

**Doubts:**

- How stable is nushell's parsed AST serialization format? If it's not stable, "send parsed AST over the wire" is brittle and cache-in-shell is the only real option.
- Does the cache persist across shell restarts? If yes, a disk-backed cache (sled, or even plain JSON) is needed. If no, in-memory is fine and restart re-parses.

### Dimension 9: Migration path from embedded StepEngine

If shell is the target, how do we get there without a big-bang rewrite?

- **Facade approach** — add an `ExecutionBackend` trait with two implementations: `EmbeddedBackend` (current behavior) and `ShellBackend` (calls shell service). dispatch_runner selects one via config. New commands get written against the shell backend; existing behavior stays until migrated.
- **Parallel run** — run both backends for the same task and compare outputs. Cheap confidence that the migration is faithful. Expensive in resources.
- **Big bang** — just rewrite at some point. Simplest to reason about, riskiest to execute.

Facade is the obvious choice. Start with the abstraction, implement embedded on top of it (refactor, not rewrite), add shell as a second implementation, flip the toggle per-environment, eventually retire embedded.

**Doubts:**

- The `EmbeddedBackend` is most of the current StepEngine code. Refactoring it behind a trait is meaningful work before we even build shell. Budget this honestly.
- How do tests work during the transition? Existing unit tests directly construct StepEngine; they'd need to switch to constructing an EmbeddedBackend and may or may not work unchanged.

### Dimension 10: Effect on the content-pipeline plan

The content-pipeline plan this file accompanies (scheduler + extraction + agent knowledge) is a large plan. The relevant pieces that intersect with dapple-shell:

- **All custom commands are still in flow.** PDF, docx, asset text/search/status/chunks, prompt send — all live in `dapple-prompt-flow::commands/` in the current-plan target state. If shell happens later, all of these migrate.
- **Scheduler and executor are already being decomposed.** The content-pipeline plan splits dispatch_runner into scheduler / executor / extractors / data_services / audit modules. The executor module is what becomes shell-adapter-compatible once shell exists — clean migration point.
- **polars / dataset work is still parked.** xlsx handling is not in the content-pipeline plan. When we revisit datasets, the shell-vs-embedded decision will be the first thing to resolve because polars is where the bloat concern originates.

So the content-pipeline plan can proceed without committing to shell, and the decomposition it performs sets up the ground for a future shell migration.

## The polars sub-problem, revisited

Let me name the specific polars decisions that stack on top of the shell decision:

**If we stay in the embedded StepEngine forever (no shell refactor):**

- Path A: polars as a Cargo dep in `dapple-prompt-flow`. Custom `dataset` built-in commands wrap polars. ~30s compile-time hit, larger binary. Matches the existing command pattern. Tasks use `dataset open`, `dataset filter`, etc. — a new verb vocabulary.
- Path B: `nu_plugin_polars` as an external plugin. Tasks use the familiar `dfr` command family that mirrors nushell's own table operators. IPC overhead per plugin call. Fights the project's built-in-commands convention. Requires plugin registration and security policy changes.
- Path C: no polars at all — use `calamine` + hand-rolled parquet conversion for ingest, DuckDB inlined for query. DuckDB has a Rust crate, works well for embedded SQL over tabular data, no IPC. Downside: DuckDB adds a large dependency, and task scripts would use SQL rather than a dataframe API.

**If we're committing to the shell refactor:**

- Path D: polars lives in `dapple-shell`. Flow doesn't carry it. Dataset commands are shell commands. No compile-time hit on flow. The "built-in vs plugin" question might re-emerge inside shell (since shell could host plugins more safely), but it's a smaller decision.

**If we're doing a phased migration:**

- Path E: polars temporarily in flow (Path A behavior), migrate to shell later when shell exists. Accept the temporary flow bloat. Commit to writing dataset commands in a migration-friendly style (explicit state in/out, no hidden coupling to flow internals).

My current lean is **Path E**, but it depends on the sequencing answer.

## Security threat model — what we're actually defending against

The user said "we would need to heavily consider security". Let me enumerate what the realistic threats are so the design decisions can be grounded in actual risk rather than abstract principle.

### Threat 1: Malicious or buggy guide-builder output

The guide-builder is an LLM-driven pipeline. The LLM could, under adversarial prompting or model error, produce a task script that:

- Exfiltrates bound secrets via `prompt send` (embed the secret in the prompt text, let the external LLM reflect it)
- Consumes excessive resources (infinite loop, memory exhaustion)
- Calls commands in ways that produce wrong outputs downstream
- Reads from unintended data sources

**Current mitigation:** parse-time security policy validates the AST. Doesn't catch exfiltration-via-`prompt send` because prompt send is an allowed command.

**Shell mitigation:** capability overlays could restrict which tasks can call `prompt send`. Outbound proxy could log and rate-limit every prompt send call. Process isolation bounds resource consumption.

### Threat 2: Supply chain risk on custom command crates

`pdf_oxide`, `polars`, `nu-command`, or any other dep could get a vuln. Today that vuln has the full address space of the flow service.

**Current mitigation:** `cargo audit` in CI, version pinning, manual review on updates. Everything is in-process so a memory-safety bug anywhere is game over for flow.

**Shell mitigation:** per-task process isolation limits the blast radius to the task's own process (which dies after the task). Cross-task damage becomes much harder unless the attacker can get the vuln to persist across the reset.

### Threat 3: Lateral movement across concurrent tasks

Two tasks running in the same flow instance concurrently share everything. A bug in task A's script execution could in principle corrupt task B's state. With nushell's memory model this is unlikely (nushell values are immutable-ish), but with custom commands that hold mutable state, the risk is real.

**Current mitigation:** we hope custom commands are well-behaved. Not formal.

**Shell mitigation:** process-per-task makes this structurally impossible. Each task's memory is its own.

### Threat 4: Dispatch state tampering

A compromised task could in principle write malicious data to `dispatch_task_attempts`, `context_data`, or `dispatch_intent_bindings`. Today, commands like `prompt send` don't touch these directly — dispatch_runner writes them after the script returns. But if a custom command did gain DB write access, it could tamper.

**Current mitigation:** custom commands don't have DB write access. Script outputs are mediated through the return value which dispatch_runner validates.

**Shell mitigation:** shell has read-only DB credentials; writes go through dispatch_runner. Mediated interface, no direct tampering path.

### Threat 5: Denial of service

A task that runs forever, allocates unbounded memory, or floods conduit with requests can take down the service.

**Current mitigation:** none that I know of at the task level.

**Shell mitigation:** per-task cgroup limits (CPU, memory, wall-clock). Can kill runaway tasks cleanly. Conduit-side rate limiting via the proxy pattern.

### Threat 6: Credential leak

Any secret the flow service has (DB creds, conduit signing key, external API keys) is visible to task commands today. A misbehaving custom command could leak them.

**Current mitigation:** trust all custom commands.

**Shell mitigation:** shell has only the creds its commands genuinely need. The conduit signing key, e.g., could live only in a conduit proxy and not be exposed to shell at all.

## Unknowns that would need verification

Before committing to anything here, several empirical questions need answers:

1. **What's the nushell EngineState initialization cost?** Benchmark: time from "start building EngineState" to "ready to execute". Affects whether fork-without-exec works and whether warm worker pools are necessary.

2. **What's the IPC overhead for a typical task?** Benchmark: round-trip time over Unix socket with msgpack, compared to direct function call. Factors into the cost of the microservice choice.

3. **How much does polars actually add to compile time and binary size?** Build `dapple-prompt-flow` with and without polars; measure cold build, warm build, and final binary size. Inputs into Path A/D tradeoff.

4. **Does nushell support dynamic overlay loading?** If yes, how do we prevent it to keep the capability model tight?

5. **What's the distribution of task profiles?** Walk the existing guides' task scripts; categorize by "single prompt send dominates" vs "multi-command pipeline". Affects IPC sensitivity.

6. **Does nushell-in-WASM work today?** The WASI isolation option is only viable if the nushell ecosystem has decent WASM support. Current state needs checking.

7. **What's in `dfr` that we actually want?** If `dfr` exposes 20 commands and we only use 5, the plugin path is less appealing than if we'd use the full surface.

8. **What's the runtime access pattern for `prompt send`?** Does it stream tokens back as they arrive, or does it block until the response is complete? Affects whether streaming IPC is needed.

9. **How much of conduit's current client code would need to move to shell?** The conduit HTTP client, the prompt template resolver, the model config resolution — which of these are already factored cleanly and which are tangled with dispatch_runner internals?

10. **Are there any custom commands today that rely on shared global state inside flow?** If yes, they're bad migration candidates and need refactoring before shell can host them.

## Doubts and caveats I want to flag loudly

### The shell refactor might be a two-year project

If the process isolation story is load-bearing (and it is, for the security pitch), and if the fork-based approach doesn't work because of EngineState costs, and if we end up needing the warm-worker-pool + reset approach, and if we then discover that reset correctness is hard to verify, and if we eventually decide that WASM is the only real answer... we could be a year in before we have anything actually more secure than today. That's not unusual for ambitious refactors, but it's worth being honest about.

Mitigation: pick a minimum-viable isolation target first (fork-per-task, accept the cost of re-initializing EngineState on each fork, measure actual latency on a simple task, see if it's acceptable). If yes, the MVP is reachable in weeks rather than months and further refinement is optional.

### The capability overlay model might not work in nushell

I'm assuming nushell's overlay system gives us a way to gate command visibility per request. I haven't verified that nushell supports this use case — loading overlays at parse time based on a whitelist, reliably preventing dynamic overlay loading, etc. If nushell's overlay model is too permissive or too rigid, we'd need to build the capability scoping differently (e.g., by constructing a custom EngineState per request with only the allowed commands registered — which is feasible but different from the overlay story).

Mitigation: prototype the capability overlay flow in isolation before committing to it as the security mechanism. If it doesn't work, the fallback is per-request EngineState with custom command registration.

### We might be solving a problem we don't have

The current StepEngine has been running task scripts in dev for months. How many security incidents have we had? Probably zero. The shell refactor is a lot of work for a threat model that might not be biting yet. Is the motivation here actual risk, or architectural elegance masquerading as security?

Mitigation: be honest about which threats we're actually mitigating. If the primary driver is "it would be cleaner", that's fine — say so, and don't overload the justification with security language.

### Polars might be the wrong primitive for the dataset use case

I've been assuming polars is the right tabular layer because it's the standard Rust choice. But:

- DuckDB with the `duckdb` crate gives you SQL over tabular data, an embedded execution engine, and first-class parquet/xlsx/csv support. It might be a better fit for tasks that want to query datasets rather than transform them.
- Arrow (the columnar format polars is built on) might be usable directly for simpler cases, without the full polars API surface.
- Calamine alone covers xlsx reading; if all we need is "read xlsx, write parquet, expose parquet to tasks", we might not need a DataFrame API at all.

The polars lean is influenced by nushell's `dfr` plugin being polars-based, but if we're not using that plugin, the polars choice is less constrained.

Mitigation: before committing polars, sketch what the `dataset` command surface should actually look like. If it's SQL-first, DuckDB might win. If it's transform-chain-first, polars wins. If it's storage-format-only (read xlsx, write parquet, hand off parquet bytes), calamine + arrow-parquet is enough.

### The "dataset is never seen by the LLM" rule might be too strict

The user's position is that xlsx and tabular data should never flow into an LLM's context. That's a useful defensive stance but might be too absolute. Realistic cases:

- A task that produces a "summary of this dataset" — means the LLM sees *some* tabular information, even if just aggregates.
- A task that asks the LLM "does this data look anomalous" — the LLM needs to see enough rows to judge.
- A task that generates SQL or polars code based on a user's natural-language question about the data — the LLM sees the schema at minimum.

Mitigation: soften the rule to "raw dataset rows don't flow into the LLM context by default; task scripts explicitly opt in when the use case requires it". Keeps the principle, allows the exceptions.

## Open questions for the follow-up interview

These are the things I'd want to drive an interview around when we come back to this:

### Sequencing
- Do you want to pause all dataset work until shell is resolved, or ship dataset-in-flow first and migrate later?
- Is there external pressure (a specific guide that needs xlsx handling, a demo date) that forces the choice?

### Isolation model
- Which isolation primitive are you targeting? Fork-per-task, warm pool, WASM, containers?
- Have you measured or estimated nushell EngineState init cost? (If not, we should before deciding.)
- What's the latency budget per task that this decision has to respect?

### Capability overlays
- Should the capability set be auto-derived from the AST, LLM-declared, human-declared, or some combination?
- Do we need to verify that nushell's overlay model actually supports capability-scoped loading before committing to it?
- What's the canonical overlay taxonomy? (e.g., pdf, prompt, summarize, dataset, guide_search, madlib, json_schema — any I'm missing?)

### Custom command migration
- Which commands move to shell first?
- Which commands are privileged enough to stay in a conduit proxy or a separate tier?
- How do we handle commands that need to call back to flow (if any)?

### Polars-specific
- Assuming shell happens, does polars live in shell?
- Assuming shell doesn't happen (or is deferred), is Path A (polars in flow) acceptable or do we prefer Path C (DuckDB) or Path E (polars temporarily in flow, migrate later)?
- Have we ruled out calamine + arrow-parquet for the MVP without a full dataframe layer?

### Dataset UX
- We had answered "separate widget entirely" for the brief form dataset slot UX. Does that stand, or does the shell question change it?
- Multi-sheet storage: we had answered "one parquet blob per sheet, indexed by sheet_name". Does that stand?
- Schema capture: "Both — inferred at extraction, author can override per-column". Does that stand?

### Conduit integration
- If shell talks to conduit, does it go direct or through a proxy?
- Where does RAG retrieval live in the shell-ful world?
- What's the auth model for shell→conduit calls?

### Transport and payloads
- Unix socket or loopback HTTP for the shell transport?
- Do we need a shared DB credential for shell, or do we pass all state over the wire?
- What's the size threshold above which inputs get referenced rather than serialized?

### Migration
- Is the facade/trait approach (`ExecutionBackend`) the right migration path?
- Do we keep the embedded backend running indefinitely or force a cutover?
- What does parallel-run look like for validation?

### Security
- What's the actual threat we're primarily mitigating? (This affects design priority.)
- Is there a compliance or audit requirement driving this?
- How much of the "secure by design" benefit are we willing to defer in exchange for faster time-to-MVP?

## Recommendation (if you asked me to pick)

I'd start with **Path E**: polars goes into `dapple-prompt-flow` now, with a deliberate covenant to write dataset commands in a way that makes them easy to migrate to shell later. Specifically:

- All dataset commands take explicit state parameters and return explicit state — no hidden coupling to flow's dispatch_runner internals.
- The commands are pure enough that moving them means copying source files and updating a Cargo.toml, not redesigning.
- At the same time, spin up a minimal `dapple-shell` scaffold with one trivial command (maybe just `echo` or a dummy `summarize`) to validate the transport, the capability overlay mechanism, and the isolation model. No production traffic. Just a smoke test so we know what the cost actually is.

Then, after the brief-flow plan lands, use the scaffolding to decide whether to commit to the full shell refactor or back away from it. Evidence, not speculation.

This path trades speed of decisiveness for safety of direction. It lets the brief-flow plan finish. It also gives you real data to decide the shell question on, instead of deciding it based on how I happened to describe the tradeoffs in this document.

But you might legitimately prefer Path A (do shell first, accept the delay) if you see the refactor as inevitable and you don't want to build technical debt in the interim. That's a call only you can make.

## Closing note

The thing I keep coming back to: **process isolation is the whole security case for shell, and we haven't validated that any isolation approach works well with nushell**. Everything else in this musing — capability overlays, custom command migration, transport choices, polars placement — is architecture, not security. The only security-load-bearing decision is "can we actually run nushell tasks in cheap isolated units". If yes, the rest of the design cascades from that. If no, the security pitch deflates and "it would be architecturally cleaner" becomes the entire justification.

I'd want to answer that one question empirically before committing to anything. Fork a child process, run a trivial nushell script in it, measure cold and warm latency. If cold is under 50ms we're fine for almost any task profile. If cold is 500ms, we need warm worker pools and the simplicity argument weakens significantly. If cold is 5s, the whole model is probably wrong and we should look at WASM or accept that the embedded model stays.

That benchmark is maybe two hours of work. I'd run it before the next interview.
