-- Migration 95: Authored ordinal for brief units/outputs + output format grouping
--
-- Two related changes, both in service of a pane that renders the way its author
-- wrote it.
--
-- 1. `ord` — the authored position of a unit within its brief.
--
--    A brief's units are authored as one ordered sequence (`brief_def.unit_names`),
--    but materialization fans them into TWO tables: non-output units land in
--    rev.brief_units, `:::brief-output`s land in rev.brief_outputs. The repo's
--    usual ordering convention (array position -> UNNEST WITH ORDINALITY ->
--    UUIDv7 id order) encodes position WITHIN one INSERT batch, so it preserves
--    authored order within each table and loses it between them: every unit id
--    sorts before every output id regardless of how the author interleaved them.
--
--    `GET /api/journeys/{id}/steps` has to render both together, so it could not
--    reconstruct the authored order and fell back to sorting exposed slots by
--    value-slot NAME (alphabetical) — nobody's intent, and the reason a client
--    could not tell which of several sibling outputs the author meant to lead.
--
--    A stored ordinal is the documented exception to "no ordinal columns" (cf.
--    rev.asset_chunks.ordinal): the position carries data semantics — it is the
--    author's layout of the pane — and it spans two tables, so no id-magnitude
--    scheme can express it.
--
--    DEFAULT 0 is deliberate. Every pre-existing row keeps ord = 0, so ordering
--    ties fall through to the existing name tiebreaker and legacy briefs render
--    EXACTLY as they do today. Authored ordinals are 1-based, so a re-ingested
--    brief never collides with the legacy sentinel; the fix lands per brief, on
--    re-ingest, with no backfill guessing at an order the old rows never stored.
--
-- 2. `display_group` / `display_format` — sibling outputs of one deliverable.
--
--    A guide that renders one artifact in several formats (a report as source
--    markdown + HTML + PDF) exposes one computed output per format. They are the
--    same deliverable, but nothing on the wire said so, and a client could only
--    guess by parsing slot names — which silently fails the moment an author
--    names a format outside the guesser's vocabulary.
--
--    Both columns are plain nullable TEXT, NOT a lookup table, and that is a
--    deliberate departure from the display_hint_id precedent next door:
--    `display_hint` switches client rendering logic, so it is a closed
--    vocabulary; these two are opaque author-chosen labels, and a seeded
--    vocabulary would reintroduce the exact "unknown format" failure the fields
--    exist to remove. Absent ⇒ ungrouped ⇒ today's behavior.
--
--    No index: nothing filters or joins on these — they ride along on reads that
--    are already scoped by brief_id.

-- ============================================================================
-- Authored ordinal (1-based; 0 = pre-migration row with no authored position)
-- ============================================================================
ALTER TABLE rev.brief_units
    ADD COLUMN ord INTEGER NOT NULL DEFAULT 0;

ALTER TABLE rev.brief_outputs
    ADD COLUMN ord INTEGER NOT NULL DEFAULT 0;

-- ============================================================================
-- Sibling-format grouping for brief outputs (UI render hints; both nullable)
-- ============================================================================
ALTER TABLE rev.brief_outputs
    ADD COLUMN display_group TEXT,
    ADD COLUMN display_format TEXT;
