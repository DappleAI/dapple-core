# Token-Needs Demo: Pre-Plan / Musing

**Status:** Pre-planning brainstorm
**Purpose:** Define the "token-needs" demo scenario for handoff to architecture-planner agent
**Created:** 2026-01-23

---

## Key Architectural Insight

**The demo is not a fake showcase - it builds real capabilities.**

- Demo-studio is a **pure API consumer** (no special demo-only code)
- Backend features (T5, embeddings, classification) are **real playbook tasks** in dapple-prompt
- A new `/sim` command is needed to trigger simulations
- Plans will be created in **both repos**:
  - `dapple-prompt/docs/plans/` - Backend capabilities (T5, embeddings, sim command)
  - `dapple-suite/docs/plans/` - Demo consumption of those capabilities

This ensures the demo doesn't make false promises about system capabilities.

---

## Demo Concept: Personal Token Budget Intelligence

**One-liner:** Take messy freeform developer activity notes, compress and classify them, simulate token consumption patterns, and surface insights about what makes a productive day.

### The User Story

Kenny keeps a freeform activity log during development - messy notes like:
- "Had tons of plans in backlog, stuffed agents all day, super productive"
- "Morning: debugging auth issue for 2 hours, finally found the race condition"
- "Kicked off 3 parallel explore agents for codebase mapping, grabbed coffee"
- "Deep in manual refactoring, no AI help needed, just vim and patience"

The demo answers: **"Where do my tokens go, and what patterns correlate with productive days?"**

### Data Source

Token usage data comes from **local Claude Code recordings** - what's already being tracked on Kenny's system. This is real usage data, not synthetic.

---

## Technical Pipeline

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        TOKEN-NEEDS PIPELINE                              │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  [Freeform Notes]                                                        │
│       │                                                                  │
│       ▼                                                                  │
│  ┌─────────────┐                                                        │
│  │ T5 Compress │  "stuffed agents all day, super productive"            │
│  │             │  → "day: high agent usage, high productivity"          │
│  └─────────────┘                                                        │
│       │                                                                  │
│       ▼                                                                  │
│  ┌─────────────┐                                                        │
│  │  Embedding  │  Generate semantic vector for compressed line          │
│  │  (Candle)   │                                                        │
│  └─────────────┘                                                        │
│       │                                                                  │
│       ▼                                                                  │
│  ┌─────────────┐                                                        │
│  │  Prototype  │  Compare against 60-80 labeled prototypes              │
│  │  Matching   │  → "agent_heavy.parallel.exploration" (0.87 conf)      │
│  └─────────────┘                                                        │
│       │                                                                  │
│       ▼                                                                  │
│  ┌─────────────┐                                                        │
│  │  QuokkaSim  │  Discrete event simulation of token flow               │
│  │  Ingest     │  Events: activity_start, tokens_consumed, etc.         │
│  └─────────────┘                                                        │
│       │                                                                  │
│       ▼                                                                  │
│  ┌─────────────┐                                                        │
│  │  Dapple     │  Present insights via Briefs                           │
│  │  Display    │  Forms for parameter adjustment                        │
│  └─────────────┘                                                        │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Why This Demo

1. **Shows T5 compression** - The messy freeform notes demonstrate T5's ability to normalize chaotic input
2. **Showcases embeddings + prototypes** - Scalable classification without LLM reasoning at inference
3. **Integrates QuokkaSim** - First demo bridging NLP classification into discrete event simulation
4. **Uses Dapple end-to-end** - Playbooks, Briefs, forms, and task execution
5. **Relatable domain** - Kenny's own workflow, easy to generate authentic sample data
6. **Practical value** - Actually useful for understanding AI-assisted development costs

---

## Label Space Design: Discovery from Data

**Approach:** Start with no predefined labels. Let categories emerge from clustering real activity data.

### Why Data-Driven Discovery?

1. **Authenticity** - Labels reflect Kenny's actual work patterns, not generic assumptions
2. **Organic growth** - Start with clusters, refine into named categories iteratively
3. **No false precision** - Avoid inventing 80 categories that don't map to reality

### Discovery Process

```
┌─────────────────────────────────────────────────────────────────┐
│  Phase 1: Collect raw activity notes (20-50 entries)           │
│     ↓                                                           │
│  Phase 2: T5 compress to normalize language                     │
│     ↓                                                           │
│  Phase 3: Generate embeddings for each compressed entry         │
│     ↓                                                           │
│  Phase 4: CLUSTER (k-means or hierarchical) to find groups      │
│     ↓                                                           │
│  Phase 5: Name the clusters based on representative entries     │
│     ↓                                                           │
│  Phase 6: These become the initial prototype sets               │
└─────────────────────────────────────────────────────────────────┘
```

### Expected Outcome

- **Initial seed**: 10-20 emergent categories from first data batch
- **Expansion**: Add new prototypes as edge cases appear
- **Final target**: 60-80 context-aware labels (grows organically)

### Hierarchical Structure (to be discovered)

The coarse/fine hierarchy will emerge from the data:
- Coarse level: ~8-12 major activity types (natural clusters)
- Fine level: Sub-clusters within each coarse category

---

## Token Budget Model

### Resource: Token Allocation

In QuokkaSim terms:
- **Resource**: `TokenBudget { allocated: u64, consumed: u64, activity_type: String }`
- **Source**: Daily token allowance (or per-task budget)
- **Stocks**: Activity-type buckets (agent_work tokens, exploration tokens, etc.)
- **Processes**: Activities that consume tokens at different rates
- **Sink**: Completed work / productivity output

### Consumption Rates (example parameterization)

| Activity Type | Tokens/Hour | Variance | Notes |
|--------------|-------------|----------|-------|
| agent_work.parallel.* | 50,000 | High | Multiple agents burn tokens fast |
| agent_work.single.coding | 15,000 | Medium | Interactive pair programming |
| debugging.llm_assisted | 8,000 | Medium | Targeted queries |
| exploration.codebase_mapping | 20,000 | High | Explore agents are hungry |
| planning.architecture | 3,000 | Low | Occasional clarifications |
| manual_coding.* | 0 | None | No AI usage |
| reviewing.ai_output | 2,000 | Low | Reading, minor edits |

### Productivity Correlation

The simulation should track:
- Total tokens consumed per day
- Activity type distribution
- Subjective productivity markers (from log notes)
- Output metrics (commits, PRs, plans completed)

Goal: Find patterns like "agent-heavy days with good backlog prep are 3x more productive per token spent"

---

## Sample Input Data

### Freeform Log Entries (what Kenny writes)

```text
9:00 Finally tackled that auth refactor, kicked off 3 parallel agents
10:30 Agents done, reviewing output, found one issue with the middleware approach
11:00 Deep in manual fixes, just me and the code, no AI
12:00 Lunch break
13:00 Back at it, exploring the new crate with an agent
14:00 Planning session for next sprint, mostly just thinking
15:00 PR reviews, 2 from the team
16:00 Stuffed 5 agents with implementation tasks, this backlog is massive
17:00 Wrapping up, marked 4 tasks done, very productive day
```

### After T5 Compression

```text
9:00 auth refactor start, 3 parallel agents
10:30 agent review complete, middleware issue found
11:00 manual code fixes, no AI
12:00 lunch
13:00 crate exploration via agent
14:00 sprint planning session
15:00 code review, 2 PRs
16:00 5 agents, implementation batch, large backlog
17:00 day end, 4 tasks complete, high productivity
```

### After Classification

```json
[
  {"time": "9:00", "kind": "agent_work.parallel.refactoring", "conf": 0.91},
  {"time": "10:30", "kind": "reviewing.ai_output", "conf": 0.85},
  {"time": "11:00", "kind": "manual_coding.refactoring", "conf": 0.88},
  {"time": "12:00", "kind": "downtime.break", "conf": 0.95},
  {"time": "13:00", "kind": "exploration.library_research", "conf": 0.82},
  {"time": "14:00", "kind": "planning.sprint", "conf": 0.89},
  {"time": "15:00", "kind": "reviewing.pr_feedback", "conf": 0.87},
  {"time": "16:00", "kind": "agent_work.parallel.implementation", "conf": 0.93},
  {"time": "17:00", "kind": "downtime.break", "conf": 0.72}
]
```

---

## Dapple Integration Points

### Playbook Structure

```
Playbook: "Token Budget Analysis"
├── Stage 1: Data Ingestion
│   └── Step: Upload/paste activity log
│       └── Task: Parse and validate log format
├── Stage 2: Processing
│   └── Step: Compress and classify
│       └── Task: T5 compression
│       └── Task: Embedding generation
│       └── Task: Prototype matching
├── Stage 3: Simulation
│   └── Step: Run QuokkaSim model
│       └── Task: Load classified events
│       └── Task: Execute simulation
│       └── Task: Generate CSV output
├── Stage 4: Analysis
│   └── Step: Present insights
│       └── Task: Calculate metrics
│       └── Task: Render Briefs
```

### BriefPack Structure

```
BriefPack: "Token Budget Results"
├── BriefGroup: "Input Summary"
│   └── Brief: Activity log preview, classification breakdown
├── BriefGroup: "Token Usage"
│   └── Brief: Charts showing token consumption by category
│   └── Brief: Daily/weekly trends
├── BriefGroup: "Productivity Correlation"
│   └── Brief: High-productivity patterns identified
│   └── Brief: Cost-per-output analysis
├── BriefGroup: "Recommendations"
│   └── Brief: Optimization suggestions
│   └── Brief: Budget planning inputs (form for adjustments)
```

---

## What the Architecture Planner Needs

### Multi-Repo Planning Strategy

This demo spans two repositories with different concerns:

**dapple-prompt (Backend Capabilities)**
- T5 compression as a Task function
- Embedding generation as a Task function
- Prototype matching/classification as a Task function
- `/sim` command for triggering QuokkaSim
- Playbook definitions for the pipeline
- API endpoints for log ingestion and results

**dapple-suite (Demo Consumer)**
- demo-studio displays Briefs showing results
- Pure API consumption - no special demo logic
- Uses standard Brief rendering for charts/insights

### Questions to Resolve

#### dapple-prompt Side

1. **T5 Model Selection**
   - Which T5 variant for compression? (t5-small, t5-base, flan-t5?)
   - Candle-compatible checkpoint availability?
   - Where does the model live? (bundled asset? download on first use?)

2. **Embedding Model**
   - Which model for semantic embeddings? (all-MiniLM-L6-v2? bge-small?)
   - Expected vector dimensionality?
   - Same deployment question as T5

3. **Prototype Storage**
   - New database table? (prototypes with label, text, embedding vector)
   - Or simpler: JSON/parquet file in assets?

4. **Classification Task**
   - Task function signature: input log line → { kind, confidence }
   - Batch processing support?

5. **Sim Command**
   - New command: `/sim <simulation-name> [params]`
   - How does it integrate with QuokkaSim?
   - Output: CSV → where does it go? (Asset? Temp file? API response?)

6. **QuokkaSim Integration**
   - Build as a crate dependency? Or subprocess call?
   - Define custom Resource for TokenBudget?

#### dapple-suite Side

1. **Brief rendering for simulation output**
   - Need chart/visualization BriefUnit types?
   - Or use markdown tables for MVP?

2. **Demo script for token-needs**
   - Follows pattern from `05-demo-playbook-execution.md`
   - Calls real API endpoints, displays real Briefs

### Deliverables (Split by Repo)

**dapple-prompt plans:**
1. `t5-compression-task.md` - T5 as a Task function
2. `embedding-classification-task.md` - Embeddings + prototypes as Task
3. `sim-command.md` - QuokkaSim integration via /sim command
4. `token-budget-playbook.md` - Playbook assembling the pipeline

**dapple-suite plans:**
1. `token-needs-demo.md` - Demo script consuming the API

---

## Answered Questions

- **MVP scope**: Classification first (T5 + embeddings + prototypes), QuokkaSim later
- **QuokkaSim familiarity**: Used it some - include some tutorial context in plans
- **Label space**: Discover from data via clustering, don't predefine

## Remaining Questions (for architecture planning phase)

1. **Sample data**: Need 20-30 realistic log entries to bootstrap clustering
2. **Productivity markers**: What signals in notes indicate productivity?
3. **Local token data**: What format is Claude Code usage data? Where does it live?

---

## MVP Phasing

**Phase 1 (MVP): Classification Pipeline**
- T5 compression task
- Embedding generation task
- Prototype storage + matching
- Basic playbook: log → compressed → classified

**Phase 2: Simulation Integration**
- `/sim` command
- QuokkaSim integration (Kenny knows basics, some tutorial context needed)
- Token budget simulation model
- Extended playbook: classified events → simulation → insights

**Phase 3: Demo Polish**
- dapple-suite demo script
- Brief visualizations for results

---

## Next Steps

1. ✅ Brainstorm complete (this document)
2. ✅ Created `/home/kenny/src/dapple/dapple-prompt/docs/musings/token-needs/` directory
3. ✅ Saved as musing document
4. ✅ **T5 Compression (Literal Prefix)**: Implemented via `summarize --prompt` flag
   - Handler: `prompt_prefix` field in `SummarizeRequest`
   - Command: `--prompt` / `-p` flag on `summarize` command
   - Service: `summarize_text_with_prefix()` method
   - T5 backend: uses custom prefix in `prepare_input()`
   - Usage: `$text | summarize --prompt "normalize this activity log:"`
5. 🔄 **Architecture Plan: Prompt Name Lookup** - Awaiting CTO approval
   - Plan: `docs/plans/summarize-prompt-name-lookup.md`
   - Changes `--prompt` to accept a Prompt NAME (not literal string)
   - Flow looks up `prompt_object_id` from `DAPPLE_METADATA.prompt_mappings`
   - Follows same pattern as `prompt send` command
   - Aligns with principle: only database Prompts allowed for public API
6. ⏳ **Architecture Plan**: Embedding endpoint + classification (dapple-prompt)
   - EmbeddingService exists (ModernBERT, 768 dims) but not exposed via API/command
   - Need `/embed` endpoint and `embed` command
   - Need prototype storage + classification logic
   - **Blocked on**: Step 5 approval
7. ⏳ **Architecture Plan**: /sim command + QuokkaSim (blocked on plan limits)
8. ⏳ **Architecture Plan**: Demo consumption (dapple-suite)
9. ⏳ Implementation follows approved plans

---

## Technical References

- **Hugging Face Embeddings Guide**: https://huggingface.co/blog/getting-started-with-embeddings
- **HF Text Embeddings Inference**: https://github.com/huggingface/text-embeddings-inference
- **HF Model Hub - Embeddings**: https://huggingface.co/models?pipeline_tag=text-embedding
- **QuokkaSim**: `/home/kenny/src/quokkasim`
- **Candle**: https://github.com/huggingface/candle (Rust ML framework)
