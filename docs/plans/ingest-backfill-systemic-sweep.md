# Ingest / Backfill — Systemic Bug-Class Sweep & Fix Plan

> Status: **plan only** (no sweep fixes executed yet). Written 2026-06-14 after
> fixing the shaggy-dog re-ingest demo bug (E1 + E2 below). The point of this
> doc is to stop fixing these one guide at a time — they are a *class*, and the
> class is enumerable.

## Context

Re-ingesting / `?backfill=true`-ing a guide intermittently produced corrupted
journeys (a multi-step stage rendering with a step missing). Root-causing the
shaggy-dog case turned up **five distinct bug classes** in the
parse→transform→**materialize**→resolve pipeline, of which only some were fixed
for the entities that happened to be reported. This sweep enumerates every site
of each class so they can be fixed structurally + guarded by an invariant test
battery instead of discovered ad-hoc.

### What is already fixed (for reference)

- **Part A** (prior PR): `patch_new_versions.sqlc` + the 5 entity patch tails
  thread input `ord`; steps/stages PATCH results are assigned **by slug**;
  `assemble_playbook_children_in_tx` has a distinctness + completeness guard;
  `validate_ingest_plan` rejects slug collisions.
- **E1** (this PR): `select_guide_playbooks_by_guide_ids`
  (`crates/dapple-data/src/query/guide_playbooks.rs`) collapses to the
  latest-non-deprecated playbook **per path** (mirrors
  `select_brief_pack_ids_for_guide.sqlc`).
- **E2** (this PR): re-ingest **deprecates superseded** playbook + brief_pack
  versions (`deprecate_superseded_versions_in_tx` in `guides.rs`), append-only
  (replaced the brief-pack hard `delete`-then-insert).
- **The ForceRepair design already exists**: under `?backfill=true`,
  `force_repair_nonce()` / `repair_hash()` (`guide_ingest.rs:878-905`)
  nonce-rewrite the task / value_slot / brief / brief_group / brief_pack /
  inference_profile / prompt hashes, so the whole task→step→stage→playbook chain
  re-versions via the Merkle cascade and a **fresh, correct version** is produced.
  Backfill repair is therefore delivered by *re-versioning* + E1/E2 resolution —
  NOT by in-place reuse-repair. (A "Part F" reuse-repair was prototyped and
  **removed as dead code**: the nonce guarantees playbooks never reuse under
  ForceRepair, so it could never fire.)

---

## The bug classes (each with concrete sites)

### Class A — positional PATCH-result assignment

**Shape:** a materializer PATCHes a batch, the SQL returns rows in `ORDER BY <id>`
(not input order), and the Rust assigns results back to inputs **by index**
instead of by a stable **key** (slug/name). A mixed batch (a row reverting to a
prior content hash → conflict arm returns an older/smaller id) re-orders the
result → version↔entity↔path mis-pairing → a sibling is dropped/cross-wired.

**Status:** tasks/steps/stages fixed (Part A, slug-keyed).

**OPEN — P0 (live bug):** `materialize_fluent_messages`
(`guide_ingest.rs` ~2329-2331) assigns by `enumerate()` index against
`upsert_fluent_messages.sqlc` (`ORDER BY nmsg.id`). A mixed fluent batch
cross-wires `message_id_map` → briefs/stages/steps reference the WRONG fluent
version → wrong display text. **Fix:** key `message_id_map` by message name from
the returned rows (mirror the tasks pattern). Verify, then ship.

**Audit-clean (keyed, but SQL still `ORDER BY <id>`):** value_slots, briefs,
brief_groups, brief_packs, inference_profiles, prompts, guide_prompts all key by
name → safe today. **Hardening:** make their patch `.sqlc` final `ORDER BY ord`
(like tasks/steps/stages/playbooks/guides) so the SQL isn't a latent trap if a
caller ever reverts to positional assignment.

### Class B — stale guide/journey → entity resolution

**Shape:** an associative junction (guide/journey → versioned entity) accumulates
multiple versions per path on re-ingest (append-only `ON CONFLICT DO NOTHING`);
the resolution query that reads it does NOT collapse to latest-non-deprecated, so
a stale (possibly corrupt) version is returned/pinned.

**Status:** `guide_playbooks` (E1+E2), `guide_brief_packs` (E2 + resolution
already collapsed) fixed. `journey_brief_packs` read-side already collapses.

**OPEN — P1:** `journey_inference_profiles` — `select_journey_inference_profiles`
returns **all** linked rows (no `latest_deprecation` filter, no
`DISTINCT ON (path)`); ingest appends with no deprecate, no `ON CONFLICT`. A
re-versioned inference profile linked to a journey accumulates → stale/duplicate
profiles. **Fix:** collapse the resolution + dedupe/deprecate on link (mirror
`guide_playbooks`).

**OPEN — P2 (defensive):** `journey_brief_packs` ingest insert has no
`ON CONFLICT` + no deprecate (read-side saves it today). Add both to match
`guide_brief_packs` so a future raw reader can't trip.

**Confirmed out of scope (non-versioned junctions):** `guide_assets`,
`guide_cards`, `guide_renders`, `guide_shared_refs` (link by asset_id / 1:1 /
cache / path, not by version).

### Class C — Normal-mode reuse perpetuates corruption (hash blindspot)

**Shape:** a parent's content hash is computed from its **authored** children,
not its **persisted** associative rows. So a dropped associative row (e.g. a
`stage_steps` row) leaves the parent hash unchanged → a Normal-mode re-ingest of
unchanged content REUSES the corrupt version (hash matches) and the corruption
persists. `?backfill=true` escapes this only because the nonce forces a fresh
version regardless.

**Blindspot inventory (parent hash does NOT capture associative membership):**
- `stage_steps` — `rev.playbook_stages`/`stage_steps` have **NO UNIQUE
  constraint**; a dropped/duplicate `stage_step` is invisible to `hash_stage`.
- `playbook_stages` — **NO UNIQUE**; dropped/duplicate stage link invisible to
  `hash_playbook`.
- `step_tasks` — **NO UNIQUE**; dropped step↔task invisible to `hash_step`.
- `brief_group_briefs`, `brief_pack_groups`, `task_prompts` — HAVE UNIQUE (so no
  duplicates) but a *drop* is still invisible to the parent hash.

**Fix direction (P2):** make associative inserts **total + guarded** so a drop is
impossible/loud rather than silent (see Class D), removing the danger of the
blindspot. The "make the hash capture persisted membership" alternative is harder
(the hash is computed pre-insert) and unnecessary if drops can't happen silently.

### Class D — silent associative-insert drop

**Shape:** `INSERT ... ON CONFLICT DO NOTHING` into an associative table whose
caller does NOT verify `submitted_count == returned_count`. A mis-resolved or
duplicate pair is silently swallowed → a child is dropped.

**Status:** `insert_stage_steps` is now guarded inside
`assemble_playbook_children_in_tx` (Part A).

**OPEN inventory (no completeness guard):**
| insert | table | UNIQUE? | guard? |
|---|---|---|---|
| `insert_step_tasks` | `step_tasks` | ❌ | ❌ (P1) |
| `insert_task_prompts` | `task_prompts` | ✅ | ❌ (P2) |
| `insert_stage_step_overlays` | `stage_step_overlays` | ✅ | ❌ (P2) |
| `insert_playbook_stage_overlays` | `playbook_stage_overlays` | ✅ | ❌ (P2) |
| `insert_guide_brief_packs` | `guide_brief_packs` | ❌ | ❌ (P2, append-only is fine post-E2) |
| `insert_prompt_fluent_messages` | `prompt_fluent_messages` | ❌ | ❌ (P2) |
| `insert_guide_embedding_{locales,tags}` | embedding junctions | ❌ | ❌ (P3, regenerated wholesale) |

**Fix direction:** (1) add a `submitted == returned` guard at each insert
wrapper (cheap, mirrors `insert_stage_steps`); (2) add the missing UNIQUE
constraints (`stage_steps`, `playbook_stages`, `step_tasks`) via migration so
`ON CONFLICT` is meaningful and duplicates are structurally impossible.

### Class E — backfill non-idempotency (by design, but unbounded)

`force_repair_nonce()` makes `?backfill=true` mint NEW versions of every entity
on **every** run (verified: a single unchanged-content backfill takes
task/step/stage/playbook from 1→2 versions). Combined with E1/E2 this is now
*correct* (resolution returns the newest, old ones deprecated), but it grows the
version DAG without bound on repeated backfills (the demo had 3 playbook
versions). **Future direction (P3):** make backfill idempotent — reuse when the
content is truly unchanged AND the persisted associative state is complete. This
depends on Class C/D being closed first (so "unchanged" is trustworthy). This is
the real "ingest + backfill fully working" end state.

---

## Prioritized fix order

1. **P0** Class A fluent cross-wire (`materialize_fluent_messages` → key by name).
   Small, live, same class as the shipped fix.
2. **P1** Class B `journey_inference_profiles` resolution collapse + link dedupe.
3. **P1** Class D guard on `insert_step_tasks` (+ UNIQUE on `step_tasks`).
4. **P2** Class D guards on remaining associative inserts; UNIQUE on
   `stage_steps` + `playbook_stages` (migration).
5. **P2** Class B `journey_brief_packs` defensive `ON CONFLICT` + deprecate.
6. **P2** Class A SQL hardening (`ORDER BY ord` on the keyed-but-fragile patch SQL).
7. **P3** Class E backfill idempotency (after C/D close).

## Test strategy — invariant battery, not one-off cases

The reason these recur is that each was tested (if at all) by a bespoke case.
Replace that with a **single invariant harness** run over EVERY fixture guide
(and a generated worst-case multi-stage/multi-step fixture), exercising
`ingest → re-ingest(mixed batch) → backfill → create journey` and asserting:

1. **No dropped/cross-wired steps:** every stage's latest version links exactly
   its authored steps (the `latest_stage_step_map` invariant, generalized).
2. **Single resolution per path:** guide resolves to exactly one
   playbook/brief_pack per chapter path; journey resolves to one
   inference_profile/brief_pack per path.
3. **No orphans:** every step version in the resolved playbook is reachable
   stage→step; no stage version linked to zero playbooks for the resolved tree.
4. **Fluent fidelity:** display text resolves to the latest fluent version
   (catches Class A cross-wire).
5. **Superseded deprecated:** after a content change, the prior versions carry a
   deprecation event.
6. **Journey end-to-end:** a journey created after backfill has `/stages` and
   `/steps` reflecting the authored structure.

Plus two review checklists (enforced in code review / a clippy-style lint if
feasible):
- **No positional PATCH assignment** — every `patch_*` result must be assigned
  via a `HashMap<key, id>`, never by index.
- **Every associative insert is guarded** — `submitted == returned`, or a UNIQUE
  + total-insert guarantee.

## Key files

- `crates/dapple-prompt-api/src/handler/guide_ingest.rs` — all materializers,
  `force_repair_nonce`/`repair_hash`, the Class A fluent site (~2329).
- `crates/dapple-data/src/query/{guide_playbooks,guides,steps,stages,step_tasks,
  task_prompts}.rs` — resolution + associative inserts.
- `crates/dapple-data/assets/sqlc/**/patch_*.sqlc`,
  `**/insert_*.sqlc`, `inference_profiles/select_journey_inference_profiles.sqlc`.
- `crates/dapple-hash/src/lib.rs` — `hash_*` (the Class C blindspot lives here).
- `crates/dapple-data/migrations/` — add UNIQUE constraints (Class D).
- `crates/dapple-prompt-api/tests/guide_ingest_test.rs` — the invariant battery.
