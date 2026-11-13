# Token-Needs Demo: Issue Summaries

**Status:** Draft - will be updated as architecture plans complete
**Purpose:** GitHub issue descriptions for implementation tracking

---

## Phase 1: Classification Pipeline

### Issue 1: T5 Text Compression Command ✅ DONE (Literal Prefix)

**Title:** `[token-needs] Add T5 compression command for log normalization`

**Summary:**
Implemented via `--prompt` flag on existing `summarize` command (accepts literal T5 prefix string).

**Implementation:**
- `summarize --prompt "normalize this activity log:"` for compression
- Handler: `prompt_prefix` field in `SummarizeRequest`
- Service: `summarize_text_with_prefix()` method
- T5 backend: uses custom prefix instead of hardcoded "summarize:"

**Usage:**
```nushell
$text | summarize --prompt "compress:"
$activity_log | summarize --prompt "normalize this activity log:"
```

**Status:** Complete - no separate command needed

---

### Issue 1.5: Summarize Prompt Name Lookup 🔄 AWAITING CTO APPROVAL

**Title:** `[token-needs] Change summarize --prompt to accept Prompt names`

**Summary:**
Architecture plan complete. Changes `--prompt` from accepting literal T5 prefix strings to accepting Prompt names that are looked up from `DAPPLE_METADATA.prompt_mappings`.

**Plan Document:** `docs/plans/summarize-prompt-name-lookup.md`

**Key Changes:**
- Flow: `summarize --prompt "name"` looks up `prompt_object_id` from `prompt_mappings`
- Flow: `SummarizeRequest` uses `prompt_object_id: Option<Uuid>` + `playbook_task_id: Option<i64>`
- Conduit: Handler resolves Prompt via database, extracts T5 prefix
- Follows same pattern as `prompt send` command

**Target Usage:**
```nushell
# In a Task, with "activity-log-normalizer" bound via task_prompts:
$text | summarize --prompt "activity-log-normalizer"
```

**Out of Scope:** Fluent message → prefix text conversion (separate "fluency" design)

**Status:** Plan complete, awaiting CTO approval

**Dependencies:** None

---

### Issue 2: Embed Endpoint + Command

**Title:** `[token-needs] Expose embedding service via API and nushell`

**Summary:**
EmbeddingService (ModernBERT, 768 dims) already exists but isn't exposed. Need endpoint and command.

**Current State:**
- `EmbeddingService` in `dapple-prompt-conduit/src/service/embedding/`
- `ModernBertBackend` with 768-dimensional embeddings
- Config via `EMBEDDING_ENABLED`, `EMBEDDING_MODEL_ID`, `EMBEDDING_DIMENSION`
- No `/embed` handler, no `embed` command

**Acceptance Criteria:**
- [ ] `POST /embed` endpoint in conduit (like `/summarize`)
- [ ] `embed` command in nushell flow (like `summarize`)
- [ ] Batch support for multiple texts
- [ ] Returns vector(s) as list of floats

**Dependencies:** None (service already exists)

---

### Issue 3: Prototype Classification

**Title:** `[token-needs] Add prototype-based classification`

**Summary:**
Classify text by comparing embeddings to labeled prototypes using cosine similarity.

**Acceptance Criteria:**
- [ ] Prototype storage (JSON asset file to start)
- [ ] Cosine similarity function
- [ ] `classify` command or nushell pipeline integration
- [ ] Returns: `{ label, confidence, nearest_prototype }`
- [ ] Initial prototype set with ~20 categories (discovered from data)

**Dependencies:** Issue 2 (embed endpoint)

---

### Issue 3: Token-Needs Classification Playbook

**Title:** `[token-needs] Create classification pipeline playbook`

**Summary:**
Define the Playbook structure that orchestrates the log → compress → classify pipeline.

**Acceptance Criteria:**
- [ ] Playbook with stages: Ingestion, Processing
- [ ] Tasks for: parse input, compress, embed, classify
- [ ] BriefPack for displaying classification results
- [ ] ValueSlots for user input (paste log, adjust parameters)

**Dependencies:** Issues 1 & 2

**Estimated scope:** TBD (pending architecture plan)

---

## Phase 2: Simulation Integration (Pending Plan Limits)

### Issue 4: QuokkaSim Integration + /sim Command

**Title:** `[token-needs] Add /sim command for discrete event simulation`

**Summary:**
Integrate QuokkaSim and add a command to run token budget simulations.

**Acceptance Criteria:**
- [ ] `/sim token-budget` command
- [ ] TokenBudget resource definition
- [ ] Consumption rate parameters by activity type
- [ ] Plan limit constraints (values TBD - awaiting numbers)
- [ ] CSV output of simulation results
- [ ] Mode-switching behavior when approaching limits

**Dependencies:** Phase 1 complete, plan limit numbers

**Estimated scope:** TBD

---

### Issue 5: Extended Playbook with Simulation

**Title:** `[token-needs] Extend playbook with simulation stage`

**Summary:**
Add simulation and analysis stages to the token-needs playbook.

**Acceptance Criteria:**
- [ ] Stage 3: Simulation (load events, run sim, generate output)
- [ ] Stage 4: Analysis (metrics, insights)
- [ ] BriefPack for simulation results and recommendations
- [ ] What-if scenario support

**Dependencies:** Issue 4

**Estimated scope:** TBD

---

## Phase 3: Demo Polish (dapple-suite)

### Issue 6: Demo Script for Token-Needs

**Title:** `[demo-studio] Token-needs demo script`

**Summary:**
Create demo script that exercises the token-needs pipeline through the API.

**Acceptance Criteria:**
- [ ] Demo script following existing patterns
- [ ] Sample data included
- [ ] Narration/walkthrough content
- [ ] Brief rendering for all result types

**Dependencies:** Phase 1 & 2 backend complete

**Estimated scope:** TBD

**Repo:** dapple-suite (not dapple-prompt)

---

## Tracking

| Issue | Phase | Status | Plan Doc |
|-------|-------|--------|----------|
| T5 Compression (literal) | 1 | ✅ Done | Via `summarize --prompt` |
| Prompt Name Lookup | 1 | 🔄 Awaiting CTO | `summarize-prompt-name-lookup.md` |
| Embed Endpoint | 1 | ⏳ Needs plan | Expose EmbeddingService |
| Classification | 1 | ⏳ Needs plan | Prototype storage + cosine similarity |
| Classification Playbook | 1 | ⏳ Needs plan | `token-needs-playbook.md` |
| /sim Command | 2 | Blocked on limits | TBD |
| Simulation Playbook | 2 | Blocked on limits | TBD |
| Demo Script | 3 | Blocked on Phase 1-2 | TBD |

---

## Notes

- **Current blocker**: `summarize-prompt-name-lookup.md` plan awaiting CTO approval
- After approval: Plan embed endpoint + classification
- Phase 2 blocked on plan limit numbers (user gathering from usage page)
- Phase 3 is in dapple-suite repo, separate planning needed
