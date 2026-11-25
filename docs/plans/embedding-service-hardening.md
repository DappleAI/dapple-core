# Embedding service hardening — truncation loss + thread oversubscription

**Status:** Phase 0 shipped (`ba7f91eb`). **Phase 1 (windowing + truncation
guard + RAG caps) implemented and deployed to dev 2026-08-02.** Phase 2's
premise was **disproven** — see §4; what shipped there is an instrument, not a
fix. Two symptoms remain genuinely open (§4 "Still open").
**Date opened:** 2026-08-02
**Branch:** `edgar-fund-support`
**Origin:** `mark-the-bdc` guide sync returning 500
`Embedding generation failed: … conduit-onnx request failed: error sending
request for url (http://conduit-onnx:8083/embed)`

---

## 1. What actually happened (measured, not inferred)

The BDC guide was **not** the cause. Its blocks max at 11,455 chars (~2,863
tokens) — the exact size that succeeded at 2026-08-01 22:07 with 162 embeddings
written. Three compounding failures in the shared embedding service killed it.

### Evidence trail

| Time (UTC) | Event |
|---|---|
| 00:18:54 | BDC version commits: 396 blocks, **0 embeddings** |
| 00:19:07 | conduit-onnx starts, warms up, idles at **2.6Gi / 12Gi** |
| 00:20:10 | `Generating embeddings for **1 texts**` |
| 00:20:13 | **OOMKilled, exit 137** — ~9.3Gi consumed in 3 seconds |
| 00:20:13 | conduit: `RAG retrieval failed … error sending request` |

The killing request was a batch of **one**, and the in-flight caller was the RAG
path — not the ingest. Guide versions confirm content committed while embeddings
did not:

```
mark-the-bdc | 2026-08-02 00:18:54 | 396 blocks |   0 embeddings   <- failing sync
mark-the-bdc | 2026-08-01 22:07:19 | 396 blocks | 162 embeddings   <- last good
```

### Root causes

1. **RAG embedded whole prompts as queries.** `rag.rs` falls back to
   `prompt_text` when a task sets no `query_hint`. EDGAR prompts on this branch
   run 17k–135k chars; a 135,591-char prompt tokenizes to ~30,000 tokens.
2. **Nothing bounded sequence length, or the batch×length² product.**
   `EMBED_BATCH_SIZE = 8` (dapple-prompt-api) bounds how MANY texts share a
   call, never how long one is.
3. **Probe timeouts killed a merely-busy pod.** Both probes used the k8s default
   `timeoutSeconds: 1`.

**Why only the BDC guide:** it is the only guide whose embed pass (~21
sequential calls, ~70s) stays in flight long enough to overlap a crash. Smaller
guides finish in the gaps.

### Measured memory envelope (12Gi limit)

| group × len² | area | outcome |
|---|---|---|
| 1 × 4,096² | 16.8M | **ok**, 14.9s |
| 1 × 8,192² | 67.1M | OOMKilled |
| 8 × 4,096² | 134M | OOMKilled |

Peak memory tracks the **product**. Bounding either factor alone leaves it
unbounded. Note `1 × 8,192²` (67.1M) died while the historical `8 × 2,863²`
(65.6M) survived — near-identical areas, opposite outcomes. That anomaly is
**explained by Phase 2 below** (thread oversubscription makes the envelope
noisy), and is the reason the budget was set conservatively.

---

## 2. Phase 0 — SHIPPED (deployed to dev, committed as `ba7f91eb` "Fix embedding problems", issue #596)

Four files. All verified live; do not re-litigate without re-measuring.

| File | Change |
|---|---|
| `crates/conduit-onnx/src/service/embedding/modernbert.rs` | `MAX_SEQUENCE_TOKENS = 4096`, `MAX_BATCH_TOKEN_AREA`, `plan_groups()` (order-preserving area-bounded grouping), extracted `run_group()`, 4 unit tests, compile-time invariant assert |
| `crates/conduit-onnx/src/service/embedding/mod.rs` | `max_sequence_tokens` + `max_batch_token_area` config, `EMBEDDING_MAX_TOKENS` / `EMBEDDING_MAX_BATCH_AREA` env overrides |
| `crates/dapple-prompt-conduit/src/service/rag.rs` | `MAX_QUERY_CHARS = 32_768`, UTF-8-safe `clamp_query_text()` + warn, 3 tests |
| `assets/k8s/registry/conduit-onnx.yaml` | Probe budgets sized for CPU-saturated inference |

**Verified:** the synthetic worst case (8 × 135,591-char texts) that OOM-killed
the pod twice now returns HTTP 200, 8 embeddings, 768-dim, 0 restarts.
Suites green: 42 (conduit-onnx) + 155 (dapple-prompt-conduit), clippy
`-D warnings` clean.

**Probe rationale (do not tighten back):** a failing *readiness* probe pulls the
single pod from the Service endpoints, so the caller's next batch hits an
endpointless Service and fails with the SAME
`error sending request for url` message as a crash — with nothing actually
wrong. A genuinely dead process still fails fast (connection refused, not
timeout), so crash detection is retained.

---

## 3. Phase 1 — Truncation is silent index loss (CORRECTNESS BUG)

### The problem

Phase 0 made over-cap text **truncate**: anything past 4,096 tokens is embedded
only up to the cap, and the remainder is **never retrievable**. For a system
whose premise is that agent prose is RAG-reachable
(`:::prose{tags:[agent]}` + auto-RAG), dropping a block's tail is data loss.

**This was initially dismissed on the grounds that "no real BDC block reaches
4,096 tokens."** That reasoning measured a moving target and was wrong. The
guide is actively growing — `accounting-primer.md` is 40,090 bytes and
`fund-primer.md` 32,108 bytes as source files today. One `:::prose` block over
the cap silently loses its tail with only a WARN line as evidence.

### The fix: window long blocks into multiple embeddings

**No migration required.** Confirmed against the live schema:

- `rev.guide_content_embeddings.chunk_text` — the column is already named for
  chunks.
- `guide_content_embeddings_block_idx` is a **plain btree, NOT unique** — many
  embedding rows per `block_id` are already legal.
- `rev.guide_embedding_tags` / `rev.guide_embedding_locales` cascade off the
  embedding `id`, so per-window tags and locales come along automatically.

The schema was designed for many rows per block; the pipeline just never emitted
more than one.

### Where

`crates/dapple-prompt-api/src/handler/guide_embeddings.rs` —
`extract_embeddable_blocks()` currently emits exactly one `EmbeddableBlock` per
guide block. Emit one per window instead.

### Design points — SETTLED (implemented 2026-08-02)

1. **Window size in chars, not tokens.** `EMBED_WINDOW_CHARS = 12_000`,
   `EMBED_WINDOW_OVERLAP_CHARS = 1_500`, in
   `crates/dapple-prompt-api/src/service/text_window.rs`. Compile-time asserts
   hold the pairing with a mirrored copy of `MAX_SEQUENCE_TOKENS` (there is no
   dependency edge between the crates, so the relationship is documented, not
   imported). **Measured live**: 47,668 chars tokenized to 9,817 → **4.86
   chars/token** on English guide prose, so a 12,000-char window is ~2,470
   tokens, ~40% under the 4,096 cap. The documented ~4 chars/token is
   conservative for this corpus.
2. **Threshold-triggered.** A block at or under the window returns exactly one
   window, byte-identical to today. Only currently-broken blocks change. The
   existing lifecycle count-equality assertion therefore still passes; it was
   reformulated to `COUNT(DISTINCT block_id)` + a `>=` floor so it stays correct
   under both regimes rather than becoming a trap.
3. **Boundary ladder**: paragraph → sentence → **line** → whitespace → hard cut.
   The line tier is an addition to the original plan: markdown tables and tight
   lists have neither blank lines nor sentence terminators and would otherwise
   fall straight to whitespace and cut mid-row. Char-index based throughout, so
   a multi-byte char is never halved.
4. **Metadata.** `chunk_ordinal` + `chunk_count` in the existing jsonb. Search
   DTOs drop the embedding row `id` but pass `metadata` through, so this is what
   distinguishes two windows of one block on the wire — no DTO change needed.
5. **Retrieval fan-out — capped in RAG only.** `MAX_WINDOWS_PER_BLOCK = 2` with
   `RETRIEVAL_OVERFETCH = 3` and a `MAX_RETRIEVAL_ROWS = 100` clamp (the clamp is
   NOT optional — `RagContext::limit` arrives off the wire unbounded and goes
   straight to SQL `LIMIT`, so over-fetching without a ceiling turns a hostile
   `limit` into a 300k-row scan). `guide_search.rs` and conduit's `search.rs` are
   deliberately **not** capped: browse surfaces where several matching passages
   of one block are useful.
6. **`retrieved_block_ids` deduped** in first-seen (= similarity) order, never
   via `HashSet::into_iter()`. Whether block-granular exclusion is still the
   right semantics is **deferred**: the mechanism is inert in production
   (nothing writes `_rag_seen_block_ids`; conduit never returns
   `retrieved_block_ids` to flow), so changing its granularity now would ship
   untested behaviour. Decide at wire-up time.
7. **Truncation is a hard ingest failure** (not in the original plan; added
   because a char→token estimate cannot be trusted for all content).
   `POST /embed` now reports `truncated_count` and the ingest returns `Err`
   **before the generation row exists** when it is non-zero, so a lossy index can
   never be committed. Scoped to ingest — RAG/search embed queries that clamp on
   purpose and persist nothing, and ignore it deliberately. This converts a
   constants-drift bug from silent content loss into a loud availability
   failure, which is the right way round to fail.

Also fixed en route: `apply_retrieval_caps` skips duplicate embedding row ids,
neutralising a **pre-existing** defect where the search queries JOIN
`guide_embedding_tags` and `guide_embedding_locales` with no `DISTINCT`, so a
block matching two requested tags already returned twice and burned two `LIMIT`
slots. The SQL-level `EXISTS` rewrite that would fix it for both search endpoints
too was **not** taken — still open.

### Landmines

- **Existing guides keep one row per block until re-ingested.** Embeddings are
  append-only per `generation_id`, so windowing only takes effect on the next
  ingest. Any "is this guide windowed?" check must key on the generation, not
  the guide.
- **`chunk_text` is what RAG renders as the excerpt.** Windows change excerpt
  size in the `# Reference Context` frame. This is likely an improvement
  (tighter, more focused excerpts) but it changes prompt shape — verify against
  a real dispatch, not just the search endpoint.
- **The ivfflat index is `lists = 100`.** Row count rises by the windowing
  factor; recall characteristics shift. Probably fine at current scale, but
  re-check search quality after the first windowed ingest.
- **Do NOT re-add guide/chapter titles to RAG headers.** The neutral
  `## Excerpt i/N` convention is deliberate (audience-voiced copy must not reach
  the conduit).

### Verification — done

**Unit (ungated, no DB, no conduit):**
- `service::text_window` — 13 tests: byte-identical under the cap; empty; **full
  coverage** (no gap, no dropped span, per-window char-slice equality against the
  source); each boundary tier; terminates on a 20,000-char boundaryless word;
  never splits `é` / `🙂` / `日本`; exact overlap; degenerate parameters.
- `handler::guide_embeddings` — under-cap block yields one byte-identical entry;
  over-cap block fans out with a planted tail sentinel present in some window
  (*that sentinel is precisely what is lost today*); `position` counts blocks not
  windows; non-embeddable/empty blocks still skipped before windowing; metadata
  carries the ordinal.
- `service::rag` — per-block cap; best windows kept in similarity order; frame
  still fills to `limit`; duplicate row consumes one slot; exclude set honoured;
  empty/zero-limit. **The zero-limit test caught a real bug**: the break ran
  after the push, so `limit: 0` returned one row.

**DB-touching (ungated, no conduit needed):**
`test_multi_window_embeddings_are_independently_retrievable` in
`guide_ingest_test.rs` — ingests a real guide, asserts the clean-slate
precondition, writes two windows of one real `block_id`, then verifies via SQL
that the plain btree really admits both, that ordinals round-trip, that each
window got its own tag row, that a **real search returns BOTH** (the tail window
is retrievable — the property broken today), and that deleting one window
cascades its junction rows while the other survives.

**Live (dev cluster, 2026-08-02):** deployed conduit-onnx + conduit + api.
A 47,668-char probe returns `truncated_count: 1` from conduit-onnx AND through
dapple-prompt-conduit; a short probe returns `0`. The pod log corroborates:
`Embedding input exceeds the sequence cap: 1/1 texts truncated from up to 9817
tokens to 4096`. Zero restarts.

**Still owed:** a gated full-pipeline ingest of an over-cap `:::prose` fixture
(body must live in `locales/en.ftl` — prose is Fluent-backed), asserting `> 1`
row and that `string_agg(chunk_text)` contains the tail sentinel. The unit +
DB tests above cover both halves separately; this would join them.

**Acceptance on the next real BDC sync:**

```sql
SELECT block_id, count(*) FROM rev.guide_content_embeddings
WHERE generation_id = (SELECT id FROM rev.guide_embedding_generations
                       WHERE guide_id = $1 ORDER BY created_at DESC LIMIT 1)
GROUP BY 1 HAVING count(*) > 1;
```

— plus no `Embedding input exceeds the sequence cap` in the conduit-onnx log
(with the guard, that condition now fails the ingest outright rather than
silently clipping).

---

## 4. Phase 2 — Thread oversubscription: **DISPROVEN 2026-08-02**

### The hypothesis, and why it was wrong

The theory was that `crates/conduit-onnx/src/service/onnx/mod.rs:212`'s
`with_intra_threads(num_cpus::get())` spawned **32 intra-op threads onto a 2-CPU
quota** (16× oversubscription), explaining the memory plateau, the 15s
inferences, and the probe starvation together.

It rested on one unverified inference: `nproc` printed 32 inside the pod, so
`num_cpus::get()` must return 32. **It does not.** Instrumented in the running
pod on 2026-08-02:

```
nproc / Cpus_allowed_list  = 32                <- affinity mask ONLY
/sys/fs/cgroup/cpu.max     = 200000 100000     <- quota: 2 CPUs
num_cpus::get()            = 2                 <- min(affinity, quota)
```

`num_cpus` 1.17 `src/linux.rs:33` calls `cgroups_num_cpus()`, which parses
cgroup-v2 `cpu.max` and cgroup-v1 `cpu.cfs_quota_us` itself. **The intra-op
thread count has always tracked the manifest's CPU limit.** There was never any
oversubscription to fix.

`nproc` and `num_cpus::get()` answer neighbouring questions and disagree exactly
here. Do not re-open a thread-count theory from a shell command — the pod now
logs its resolved thread count at startup, and that log line is the instrument.

### What shipped instead

Not a fix — an instrument and a knob:

- `intra_op_threads()` in **both** twin `onnx/mod.rs` files (kept byte-identical;
  `md5sum` both paths is the drift check), logging the resolved count and its
  source at every session build.
- `ONNX_INTRA_THREADS` override, deliberately **not** clamped to the quota, so a
  future oversubscription theory can be *tested* against the running pod rather
  than inferred.
- 5 unit tests on the pure resolution policy.
- A doc block on `intra_op_threads` recording the disproof so nobody re-derives it.

### Consequences — two of them matter

1. **Phase 0's constants were NOT "fitted to a thread-thrashed pod."** They are
   the real envelope and will not loosen. `MAX_SEQUENCE_TOKENS = 4_096` stands,
   so Phase 1 windowing is the permanent answer to long blocks, not a stopgap,
   and `EMBED_WINDOW_CHARS = 12_000` is sized correctly against it.
2. **The plateau and the probe starvation are still open**, and now have no
   covering explanation. So is the §1 anomaly (`8 × 2,863²` survived while
   `1 × 8,192²` died).

### Still open — leading candidates, untested

- **ORT CPU arena allocator.** ONNX Runtime's default CPU arena grows and does
  not return freed blocks to the OS, which fits a pod sitting at 8,767Mi with
  CPU at 1m (idle, not working) far better than per-thread scratch ever did.
  Investigate arena config / `with_memory_pattern`.
- **`spawn_blocking`.** `session.run` is synchronous CPU work inside an async
  handler, and tokio defaults its worker count to the same 2 CPUs — so a 15s
  inference can starve `/api/health_check` with no oversubscription required.
  This was "maybe unnecessary after the thread fix"; with the thread theory dead
  it is the **leading** fix for the probe symptom.

### Landmines

- **The module is DUPLICATED across two crates**, byte-identical:
  `crates/conduit-onnx/src/service/onnx/mod.rs` and
  `crates/dapple-prompt-conduit/src/service/onnx/mod.rs`. Patch both; conduit-onnx
  has zero first-party path deps, so there is no shared crate to hoist into
  without inventing a dependency edge.
- **conduit-onnx is hostPath on Linux** (Docker-only on macOS), so deploys need
  no commit: `nu assets/nu/k8s/deploy_dapple_core.nu --svc conduit-onnx`.

---

## 5. Order

Phase 2 was to run first so its numbers could size Phase 1's window. That
rationale is void — the thread theory is disproven and the cap is not moving, so
`EMBED_WINDOW_CHARS = 12_000` (~3,000 tokens at ~4 chars/token) is final against
`MAX_SEQUENCE_TOKENS = 4_096`. **Phase 1 proceeds independently.**

## 6. Reproduction assets

Probe payloads (regenerate as needed; scratchpad is session-scoped):

```nushell
# single oversized text — the shape that OOM-killed the pod
let unit = "The BDC reported non-accrual loans across its portfolio companies. "
let text = (0..2100 | each { $unit } | str join "" | str substring 0..135591)
{ texts: [$text] } | to json | save -f probe.json

# full batch — the worst case Phase 0 now survives
{ texts: [$text $text $text $text $text $text $text $text] } | to json | save -f probe8.json
```

```nushell
use ($nu.default-config-dir | path join "modules" | path join "dapple_env_profiles") dapple_sandbox_tools
overlay use dapple_sandbox_tools as dapple_env
kubectl port-forward -n dapple-sandbox svc/conduit-onnx 18083:8083
# then, with the internal key from secret conduit-internal-auth / internal-key:
curl -s -X POST http://127.0.0.1:18083/embed -H "Content-Type: application/json" \
  -H "X-Internal-Key: $KEY" --data-binary "@probe8.json"
```

Health checks that mattered during diagnosis:

```nushell
kubectl get pods -n dapple-sandbox -l app=conduit-onnx \
  -o "custom-columns=NAME:.metadata.name,READY:.status.containerStatuses[0].ready,RESTARTS:.status.containerStatuses[0].restartCount"
kubectl get pods -n dapple-sandbox -l app=conduit-onnx \
  -o jsonpath="{.items[0].status.containerStatuses[0].lastState}"   # OOMKilled vs Error
kubectl top pod -n dapple-sandbox -l app=conduit-onnx --no-headers  # baseline 2.5Gi
kubectl get events -n dapple-sandbox --sort-by=.lastTimestamp       # probe failures
```

**Timestamp trap:** pod logs are UTC; the host is BST (UTC+1). A `date now`
rendered without conversion reads one hour ahead and makes a current crash look
stale.
