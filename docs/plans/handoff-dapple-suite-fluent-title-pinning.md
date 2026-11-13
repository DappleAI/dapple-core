# Handoff → dapple-suite: Fluent-referenced display titles (brief/group/pack/stage/step)

**From:** dapple-prompt
**Date:** 2026-05-31
**Companion:** `docs/plans/dapple-hash-crate.md`, `~/src/dapple/dapple-suite/docs/plans/content-hash-pinning.md`
**Impact on dapple-suite:** **Low.** No code changes required. Wire contract is unchanged. A latent
display bug is fixed for free. One coordinated server-side re-ingest changes derived-object version
ids/hashes once.

---

## TL;DR

dapple-prompt stopped **baking** display text (brief/group/pack titles, stage/step descriptions) into
entity rows and now **references the exact Fluent message version** (`*_fluent_message_id`), resolving
it live at read time. This fixes a regression where a brief's title rotted to its slug
(`Define Your Claims and Research Scope` → `evidence-architecture-…-brief`) on every re-version.

For dapple-suite:

- ✅ **No model/codegen changes.** The REST DTOs are byte-for-byte the same — `Brief.title`,
  `BriefGroup`/`BriefPack` titles, and stage/step `description` are still resolved **strings**. The new
  `*_fluent_message_id` columns live server-side only (we chose server-side resolution to keep the wire
  contract stable). OpenAPI spec is unchanged → `tools/generate-models/` is a no-op.
- ✅ **No `dapple-hash` / `dapple-sdk` bump required.** The shared `dapple-hash` crate is **unchanged**
  (function signatures identical). Only dapple-prompt's *ingest call-sites* pass different inputs now.
  You compute only `hash_guide_directory` (`api::discovery::compute_guide_content_hash`), which we did
  **not** touch — guide-directory hashing is byte-identical.
- ✅ **You get a bug fix for free.** Anywhere Studio/web/desktop/tui renders a brief title, stage, or
  step description, slug-rotted values will now show the correct human text and stay correct across
  re-ingests.
- ⚠️ **One-time churn:** after the coordinated re-ingest (below), the **content_hash and version id of
  every brief / brief_group / brief_pack / stage / step changes once** (their hashes now fold in the
  Fluent body). Guide-level `content_hash` is **unchanged**. Journeys are **unaffected** (they pin
  `guide_id`).

---

## What changed server-side (context)

- **Schema (migrations 48 + 49):** `briefs`/`brief_groups`/`brief_packs` gained
  `title_fluent_message_id` + `description_fluent_message_id`; `stages`/`steps` gained
  `description_fluent_message_id` (FK → `rev.fluent_messages`). The old `title`/`description` text
  columns are **retained but nullable** — the brief-unit hybrid (`label_text` + `label_fluent_message_id`).
  A `STABLE` SQL helper `rev.fluent_body(msg_id, locale)` resolves a reference to its body.
- **Reads** resolve `COALESCE(rev.fluent_body(ref,'en'), text, name)` in SQL, so DTOs keep returning a
  resolved `title: String` / `description: String`.
- **Ingest** sets `*_fluent_message_id` from the Fluent message keyed by the entity name, in **both**
  the INSERT and PATCH arms (the PATCH arm was the one baking the slug).
- **Content hashing (the source of the one-time churn):** because the model is append-only, an entity
  must re-version to re-pin an edited Fluent message. So `hash_brief`/`hash_brief_group`/
  `hash_brief_pack`/`hash_stage`/`hash_step` now fold the display Fluent message's `content_hash` into
  the entity's own hash. **This is a call-site change in dapple-prompt ingest, not a `dapple-hash` API
  change** — you are unaffected because you don't compute these hashes.

---

## Action items for dapple-suite

Most boxes are "confirm none / no-op," intentionally:

- [ ] **Confirm you don't independently compute derived-object hashes.** A grep for
  `hash_brief`/`hash_brief_group`/`hash_brief_pack`/`hash_stage`/`hash_step` should return **nothing**
  in dapple-suite (we found only `hash_guide_directory` use, which is untouched). If that's still true,
  there is nothing to mirror.
- [ ] **Confirm no local cache/pin is keyed on a derived-object `content_hash` or version `id`.** Guide
  pins (`/guide pin`/`status`/`diff`/`promote`, TOML manifests) use the **guide** `content_hash`
  (`hash_guide_directory`), which is unchanged — those are fine. We found only **display** usages of
  `brief.content_hash` (web/desktop/tui `brief_view`), which self-update from server responses. If you
  have anything that *persists* a brief/stage version id or hash across sessions, plan to refresh it
  after the re-ingest.
- [ ] **Re-run model codegen as a sanity check** (`tools/generate-models/`) against the current
  dapple-prompt OpenAPI spec — expect a **no-op diff**. If it produces a diff, stop and ping us (would
  mean an unintended DTO change).
- [ ] **Visual check in Studio** after the coordinated re-ingest: brief titles, stage names, and step
  descriptions that previously showed slugs should now show the authored Fluent text.

Nothing else. No SDK version change, no new DTO fields, no auth/route changes.

---

## The coordinated re-ingest ("re-version the world")

This is primarily a **dapple-prompt/ops** task, listed here so you know the timing:

- After the dapple-prompt change deploys, each guide must be re-ingested through the derived-object
  materializer to populate the new references — a `PATCH /api/guides/{path_id}/ingest?backfill=true`
  per guide (a plain no-op re-sync that returns 409 does **not** re-version derived objects). The
  dapple-prompt team will run this sweep.
- After the sweep: every brief/group/pack/stage/step has a fresh version id + content_hash; titles
  resolve from Fluent. Existing journeys keep resolving against their pinned `guide_id` (soft-deprecation
  continuity), so in-flight work is not disrupted; new and old versions coexist.
- If dapple-suite owns/maintains any guides, you can trigger the same `?backfill=true` re-ingest for
  yours, or let the ops sweep cover them.

---

## Known limitation (so nobody files a bug)

Brief/group/pack **descriptions** come from a Fluent `.description` **attribute**, which the server does
not persist as a queryable body — so descriptions are still **baked** (now consistently, in both ingest
arms, so they no longer rot). Only **body** text is reference-pinned: brief/group/pack **titles** and
stage/step **descriptions**. Editing only a `.description` attribute (without a body change) will not
re-version the entity. This is documented in
`crates/dapple-prompt-api/AGENTS.md` → "Display titles/descriptions are Fluent-referenced".

---

## Where the change lives (for reviewers)

- Migrations: `crates/dapple-data/migrations/00000000000048_*.sql`, `…049_*.sql`
  (incl. `rev.fluent_body()`).
- Data layer: `crates/dapple-data/{src/model,src/query,assets/sqlc}/{briefs,brief_groups,brief_packs,
  stages,steps}/…`
- Ingest + Merkle: `crates/dapple-prompt-api/src/handler/guide_ingest.rs`
  (`materialize_fluent_messages` now returns `(message_id_map, message_hash_map)`;
  `materialize_brief_system` / `materialize_single_playbook`).
- Principle docs: `crates/dapple-data/AGENTS.md` → "Versioned object references — pin exact versions";
  `CLAUDE.md` → "Content-Addressable Versioning".
- Regression test: `crates/dapple-prompt-api/tests/guide_ingest_test.rs` →
  `test_brief_title_pins_fluent_and_survives_reingest`.

Questions → dapple-prompt team.
