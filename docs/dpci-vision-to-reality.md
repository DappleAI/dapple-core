# DPCI: Vision → Reality

*An internal enablement guide for the business-development team.*

> **Purpose.** The original **DPCI (Dual Phase Context Injection)** document described where we were headed. This guide maps each of those promises onto what `dapple-prompt` actually does today — so you can speak about the product accurately and confidently. Where reality went *further* than the vision, we say so. Where something is still ahead of us, we mark it **Roadmap** plainly. Nothing here is meant to be shared verbatim with customers; it's the honest internal picture.

---

## TL;DR

DPCI promised a system that turns open-ended inputs into structured, high-quality outputs by **framing intent** and then **injecting only relevant context** at the right moments — instead of dumping everything into a prompt and hoping.

We built that. But the shape changed in a way that makes it stronger:

- The "Task Packet" isn't a document the system fills out and carries around. It's a **view composed just-in-time, for one task, at the moment of execution.** Our execution service (the *conduit*) assembles exactly the right context — model, persona, the relevant data slots, retrieved reference material — *at the point of use*, then throws away the rest.
- The "two phases" became **two axes**: the guide author **frames** the journey once, and the runtime **re-runs the framing → collect → specify → refine cycle at every step**, relevance-scoped each time.
- The headline DPCI promise — *hide the noise so the model isn't polluted* — is real and enforced at the database level (our `mask` overlay).

A few capabilities the original doc described as automatic are, today, **deliberate design choices or roadmap items** — most importantly cost-optimized model selection. Those are called out in §6 and §7.

---

## 1. Vocabulary bridge

The DPCI vocabulary was a forward-looking sketch; it doesn't appear in the product. Here's the translation:

| DPCI term | What it is in the product | Plain meaning |
|---|---|---|
| Task Packet | **Dispatch** + per-step context, composed at the conduit | The frame for one unit of work — built on demand, not stored whole |
| Injection point(s) | Per-step **overlay-scoped injection** | Context is injected fresh at *every* step, not at 4 fixed points |
| Framing | **Playbook** + **InferenceProfile** + stage/step descriptions | Who the work is for, the persona/tone, and the objective |
| Attention-aware data collection | **Debrief** over **Briefs** (forms) | The interview that gathers what the task needs |
| Assignment / options | **generate → pick → refine** steps | The system proposes options; the user picks; a follow-up refines |
| Syllabus / blueprint | **Playbook → Stages → Steps** | The authored workflow |
| Hide context | **`mask` overlay** | Structurally remove irrelevant context so it can't pollute the prompt |

The real product nouns, for reference: **Playbook, Journey, Stage, Step, Brief/Debrief, Dispatch, overlays (`expose`/`mask`), InferenceProfile, RAG, directives.**

---

## 2. The one big idea: two axes, not two phases

DPCI's "phases" collapsed two different things into one straight line. The accurate picture has **two axes**, and that's the whole pitch:

**Authoring axis — done once, by the guide author.**
The author *frames* the journey: picks the Playbook, sets the InferenceProfile (persona/tone), writes the Briefs that collect input, lays out the Stages and Steps, and declares which data each step should see (`expose`) or be shielded from (`mask`).

**Runtime axis — done again at every single step, by the conduit.**
When a task fires, the conduit composes the frame *just-in-time at the point of use*:

1. pull the running context for this step,
2. **resolve which data slots are visible** here (the `expose`/`mask` overlay scope),
3. render the prompt against *only* those slots,
4. retrieve and prepend relevant reference material (RAG),
5. prepend the live task context,
6. route to the right model for the job.

The final prompt the model sees is layered deliberately: `[task context] → [retrieved reference] → [authored instructions] → [the user's actual request]`.

So "Dual Phase" undersold it. It's not two phases — it's **one framing discipline applied once at authoring and then re-applied, relevance-scoped, at every step of execution.** That's what "injection at the point of use" means, and it's why context stays clean.

---

## 3. Phase-by-phase

Each phase below is: **what DPCI promised → what we built → why the real thing is stronger.**

### Phase 0 — Framing

- **Promised:** a "Task Packet" holding role, objective, tone, success criteria, and bounds, compiled up front before the model reasons.
- **Built:** there is no single packet object. The frame is distributed and assembled per task: **persona/tone** comes from the InferenceProfile, **objective** from the task/stage/step descriptions, **success criteria** from the step's required outputs, and **bounds** from the overlay scope and step parameters. The conduit pulls these together at execution time into the prompt the model actually sees.
- **Why stronger:** a packet compiled up front goes stale and travels with baggage. Composing the frame *at the point of use* means every step gets a frame that's current and scoped to exactly that step — nothing more.

### Phase 1 — Attention-aware data collection

- **Promised:** two modes — (a) the model dynamically generates scoped questions from gaps in context, and (b) pre-authored forms (fields, dropdowns, file upload). Plus pulling from external sources (APIs, web search).
- **Built:**
  - **Pre-authored forms — fully built.** The Brief system (BriefPack → BriefGroup → Brief → BriefUnit) supports text, textarea, select, radio, checkbox, number, date, and **file upload**, served as the "debrief" interview and filtered per step by overlays.
  - **Dynamic questioning — built, but deliberately inverted.** Rather than have the model interrogate the user with open-ended questions, the model **pre-fills bounded selections**: a step *generates* candidate options, each with a description explaining what the choice implies, and the user simply **picks**. The LLM does the generative heavy lifting; the user's input stays constrained. This was a deliberate guardrail — letting users type freely into an LLM proved problematic at a deeper level than we first expected, so we replaced open-ended prompting with author-bounded, LLM-authored choices. (The cost: it asks more of the guide author to keep things on rails.)
  - **External sources — architecturally available, connectors are Roadmap.** Task scripts run in a Nushell engine that can already make outbound HTTP calls under the default policy, so external data *can* enter a step today. We have not shipped curated connectors yet (a managed web-search command is post-demo). See §7.
- **Why stronger:** the inversion is the point. "The model asks; you pick from explained options" is safer and more repeatable than "you type and hope," and it's how we avoid the failure mode DPCI's relevance-first thesis was really worried about.

### Phase 2 — Assignment specification

- **Promised:** generate a "syllabus" from a "blueprint"; select the best model/tool by policy, capability, and **cost**; present **3–5 options** the user chooses among.
- **Built:**
  - **Blueprint → syllabus — built.** The **Playbook** is the blueprint; its **Stages and Steps** are the syllabus.
  - **Options — built as `generate → pick → refine`.** A step generates candidate options; the user picks one; a follow-up step refines it. *Note for accuracy:* the **number of options is decided by the guide author, not fixed at 3–5**, and the platform does **not** rank or auto-select a "best" option — it presents what the author's task produced.
  - **Capability matching — built.** Work is routed to the right engine by model lineage: general LLM, text-to-speech, summarization, masked-language tasks, and embeddings.
  - **Cost-aware selection — Roadmap (see §6).** Today the model is chosen by availability (Claude when configured, else a self-hosted model). Cost is handled as a *measured, versioned* discipline, not an in-the-moment pick.
- **Why stronger:** routing by capability across multiple modalities (not just "an LLM") is broader than the doc implied. And our cost philosophy — *learn the cost/quality frontier across guide versions and pin the winner* — is more defensible than a per-request cost guess, even though the automation behind it is still being built.

### Phase 3 — Refinement

- **Promised:** after the user selects, run a final survey for more context, pack the most salient context in, inject the right info at the right time, and **hide context where applicable to avoid pollution.**
- **Built:**
  - **Refine step — built.** It's the third beat of generate → pick → **refine**.
  - **Hide context — built, and it's the crown jewel.** The `mask` overlay structurally removes a data slot from a step's view — it's enforced in the data layer, so a masked slot is absent from both the operator's form *and* the model's prompt. This is exactly DPCI's "hide the noise" promise, actually shipped.
  - **Pack salient context — built via two mechanisms.** Overlay scoping (only the relevant slots are visible) plus RAG ranking (retrieve the most semantically relevant reference chunks, dedup against what earlier steps already used, prepend them). *Accuracy note:* selection is curation-by-author plus similarity ranking; there is no automatic token-budget trimming.
- **Why stronger:** "hide context to avoid pollution" is usually a slide-deck aspiration. Here it's a database-enforced guarantee, declared per step by the author.

---

## 4. Where we over-delivered (use these)

Things the original DPCI doc didn't capture, that are real today and worth talking about:

- **Database-enforced relevance *and* noise control** via `expose`/`mask` overlays — per step.
- **Iterative, branching execution.** Workflows aren't a straight pipeline; directives support loop, retry, branch, skip, and fail. (This is the real reason "Dual Phase" undersells it.)
- **Auditable context as infrastructure.** Content is content-addressed, append-only, and versioned with a Merkle structure — any change re-versions cleanly, and history is preserved.
- **Multi-modal capability routing** — LLM, text-to-speech, summarization, masked-language, embeddings — not just chat.
- **Typed data slots with write-once-per-dispatch bindings** (latest-wins across re-runs) — predictable, traceable data flow.
- **Batch-native by design.** The API is set-based throughout (operations take and return collections), and a *dispatch is a batch of one or more steps* executed with controlled concurrency.
- **Multi-tenant cryptographic isolation** between the internal and customer-facing API surfaces.

---

## 5. Honest roadmap (state as direction, not as shipped)

- **Cost-optimization loop.** The substrate exists — multiple models of varying cost, the versioning system, and a usage-event schema. Still to wire: per-prompt model selection driven by InferenceProfiles (currently a stub); **off-peak / deferred scheduling** and **provider async-batch endpoints** to run non-interactive work more cheaply; **automated A/B** across guide versions; and **usage-metrics capture/aggregation** to let cost/quality "settle over time." *(Batch execution itself is already native — what's roadmap is deferring it to cheaper windows.)*
- **External / web retrieval connectors.** HTTP from task scripts works today under the default policy, but curated connectors — e.g. a managed web-search command — are post-demo.

---

## 6. How to talk about this (accuracy guardrails)

To keep us honest with customers, avoid these specific overclaims:

- ❌ "The system automatically picks the cheapest capable model in the moment." → Cost optimization is a **measured, versioned** discipline; automatic selection is roadmap.
- ❌ "The system shows the best 3–5 options." → Option count is author-defined and there's no automatic ranking; say "the system proposes options and the user picks."
- ❌ "We don't really do batch." → **We're batch-native.** Only *off-peak scheduling* for cost is roadmap.
- ❌ "It searches the web for you." → External retrieval is *architecturally available*; shipped connectors are roadmap.
- ✅ Safe, strong claims: *injection at the point of use; database-enforced context hiding; bounded LLM-authored selections instead of open-ended prompting; multi-modal routing; auditable, versioned context.*

---

*Source of truth: this guide reflects `dapple-prompt` as read on 2026-06-05. Before sharing or quoting specifics, re-verify the roadmap items against the code (the cost-selection and metrics paths are the ones most likely to change as they get wired).*
