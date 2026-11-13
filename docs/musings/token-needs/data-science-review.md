# Token-Needs Demo: Data Science Review Request

**Purpose:** Validate the approach before implementation
**Audience:** Data scientists
**Ask:** Is this methodology sound enough for a compelling demo?

---

## What We're Building

A pipeline that takes **freeform developer activity logs** and:
1. Compresses/normalizes the text
2. Classifies each entry into activity categories
3. Feeds classified events into a discrete event simulation
4. Models token budget consumption with real plan limits as constraints

**End goal:** Surface insights about token consumption patterns and their correlation with productivity.

---

## The Pipeline

```
Raw Text → T5 Compression → Embedding → Prototype Matching → Classification → Simulation
```

### Step 1: Text Compression (T5)

**What:** Use a T5 model to compress verbose, informal log entries into normalized summaries.

**Example:**
- Input: `"Had tons of plans in backlog, stuffed agents all day, super productive"`
- Output: `"high agent usage, large backlog, high productivity"`

**Model:** FLAN-T5-base or T5-small (Candle/Rust inference)

**Why not skip this?** Raw logs have high variance in phrasing. Compression normalizes language before embedding, reducing noise in the vector space.

**Searchable terms:** T5, text-to-text transfer transformer, sequence-to-sequence compression, FLAN-T5

---

### Step 2: Semantic Embedding

**What:** Generate a dense vector representation of each compressed log entry.

**Model candidates:**
- `all-MiniLM-L6-v2` (384 dimensions, fast)
- `bge-small-en-v1.5` (384 dimensions, strong retrieval performance)
- `e5-small-v2` (384 dimensions, good general-purpose)

**Output:** A 384-dimensional float vector per log entry.

**Searchable terms:** sentence embeddings, sentence transformers, dense retrieval, semantic similarity

---

### Step 3: Prototype-Based Classification

**What:** Classify entries by finding the nearest labeled prototype in embedding space.

**Approach:**
1. Maintain a set of **labeled prototype entries** (60-80 examples across categories)
2. Each prototype has: `{ text, embedding, label, label_hierarchy }`
3. For new entries: compute cosine similarity to all prototypes
4. Assign the label of the highest-similarity prototype
5. Use similarity score as confidence

**Example prototypes:**
| Label | Prototype Text |
|-------|---------------|
| `agent_work.parallel.refactoring` | "3 parallel agents, code refactor" |
| `manual_coding.debugging` | "manual debugging, no AI assistance" |
| `exploration.codebase` | "explore agent, codebase mapping" |
| `planning.architecture` | "architecture planning session" |

**Classification output:**
```json
{
  "kind": "agent_work.parallel.refactoring",
  "confidence": 0.87,
  "nearest_prototype": "3 parallel agents, code refactor"
}
```

**Why prototypes instead of a trained classifier?**
- No training data collection phase needed
- Easy to add/modify categories by adding prototypes
- Interpretable: "you matched this example"
- Good enough for ~20 categories with clean separation

**Searchable terms:** prototype-based classification, nearest neighbor classification, few-shot learning, semantic similarity classification, exemplar-based learning

---

### Step 4: Label Discovery (Bootstrap Phase)

**What:** We don't predefine categories. We discover them from data.

**Process:**
1. Collect 30-50 raw activity log entries
2. Compress all entries via T5
3. Generate embeddings for all compressed entries
4. Run clustering (k-means or hierarchical agglomerative)
5. Inspect clusters, name them based on representative entries
6. Select 2-3 entries per cluster as initial prototypes

**Why?** Ensures labels reflect actual work patterns rather than imagined categories.

**Searchable terms:** unsupervised clustering, k-means, hierarchical clustering, cluster labeling, data-driven taxonomy

---

### Step 5: Discrete Event Simulation (QuokkaSim)

**What:** Model token consumption as a resource flow with constraints.

**Framework:** QuokkaSim (Rust-based discrete event simulation, similar to SimPy)

**Model structure:**
```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ Token       │────▶│  Activity   │────▶│ Completed   │
│ Budget      │     │  Processes  │     │ Work        │
│ (Source)    │     │  (by type)  │     │ (Sink)      │
└─────────────┘     └─────────────┘     └─────────────┘
      │                    │
      │                    ▼
      │            ┌─────────────┐
      └───────────▶│ Plan Limit  │
                   │ (Constraint)│
                   └─────────────┘
```

**Key dynamics:**
- Each activity type has a **consumption rate** (tokens/hour) with variance
- **Plan limits** are hard constraints (simulation halts when exhausted)
- **Mode switching:** when approaching limits, behavior changes (shift to low-consumption activities)
- **Backlog accumulation:** planning during low-token periods creates queued work

**Simulated scenarios:**
1. **Baseline:** Run with current usage patterns
2. **What-if:** "What if I shifted 20% of exploration to off-peak?"
3. **Forecast:** "At this rate, when do I hit the limit?"

**Searchable terms:** discrete event simulation, resource modeling, stock and flow models, SimPy (Python equivalent), system dynamics

---

## Questions for Data Science Review

### Methodology

1. **T5 compression before embedding** — Does this make sense, or should we embed raw text directly? The intuition is that compression reduces noise, but it might also lose signal.

2. **Prototype-based classification** — For ~20-30 categories with good semantic separation, is nearest-neighbor in embedding space reasonable? Any concerns about:
   - Confidence calibration (is cosine similarity a good proxy for confidence?)
   - Decision boundaries (what happens at category edges?)
   - Prototype selection bias?

3. **Clustering for label discovery** — Is k-means appropriate here, or would hierarchical clustering better capture the natural hierarchy we expect (e.g., `agent_work.parallel.*` vs `agent_work.single.*`)?

### Model Selection

4. **Embedding model choice** — Any preference between MiniLM, BGE, E5 for this use case (short informal text, ~20-30 categories)?

5. **T5 variant** — T5-small (60M params) vs FLAN-T5-base (250M params) for compression quality vs. inference speed?

### Evaluation

6. **How would we validate the classification?** Options:
   - Manual spot-check of 50 random classifications
   - Hold out some prototypes and measure retrieval accuracy
   - Track user corrections over time

7. **Simulation validity** — Any suggestions for validating the simulation model against real token usage data?

---

## Sample Data

### Raw Input
```
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

### After Pipeline
```json
[
  {"time": "9:00", "compressed": "auth refactor, 3 parallel agents", "kind": "agent_work.parallel.refactoring", "conf": 0.91, "tokens_est": 15000},
  {"time": "10:30", "compressed": "agent review, middleware issue", "kind": "reviewing.ai_output", "conf": 0.85, "tokens_est": 500},
  {"time": "11:00", "compressed": "manual fixes, no AI", "kind": "manual_coding.refactoring", "conf": 0.88, "tokens_est": 0},
  {"time": "12:00", "compressed": "lunch", "kind": "downtime.break", "conf": 0.95, "tokens_est": 0},
  {"time": "13:00", "compressed": "crate exploration, agent", "kind": "exploration.library", "conf": 0.82, "tokens_est": 8000},
  {"time": "14:00", "compressed": "sprint planning", "kind": "planning.sprint", "conf": 0.89, "tokens_est": 1000},
  {"time": "15:00", "compressed": "PR review, 2 PRs", "kind": "reviewing.code", "conf": 0.87, "tokens_est": 0},
  {"time": "16:00", "compressed": "5 agents, implementation batch", "kind": "agent_work.parallel.implementation", "conf": 0.93, "tokens_est": 25000},
  {"time": "17:00", "compressed": "day end, high productivity", "kind": "meta.day_summary", "conf": 0.72, "tokens_est": 0}
]
```

---

## What "Success" Looks Like

For the demo to be compelling, we need:

1. **Classification accuracy > 80%** on manual review of 50 samples
2. **Simulation predictions within 20%** of actual daily token usage
3. **Interpretable outputs** — user can see why an entry was classified a certain way
4. **Actionable insights** — at least one non-obvious pattern surfaces (e.g., "parallel agent work before planning sessions correlates with 40% higher next-day productivity")

---

## References

- **Sentence Transformers:** https://www.sbert.net/
- **T5 Paper:** https://arxiv.org/abs/1910.10683
- **FLAN-T5:** https://arxiv.org/abs/2210.11416
- **Prototype Networks (few-shot learning):** https://arxiv.org/abs/1703.05175
- **QuokkaSim:** `/home/kenny/src/quokkasim` (local, Rust discrete event sim)
- **SimPy (Python equivalent):** https://simpy.readthedocs.io/
