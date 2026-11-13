-- Migration 98: Authored icon-name set for stages
--
-- Migration 97 gave a stage a `kind` — a closed vocabulary naming WHAT a stage
-- is. It cannot answer WHICH stage the operator is on, and should not try:
-- mark-the-bdc has fifteen stages and nine of them are honestly `analyze`, so a
-- kind-only rail draws nine identical discs. Distinguishability *within* a kind
-- is authored knowledge with no client-side substitute. See
-- docs/plans/dapple-ui-stage-icon-set.md.
--
-- This is the deliberate INVERSE of `kind`, and the inversion is the point:
--
--   * No lookup table, no enum, no ingest downgrade. The vocabulary belongs to
--     the drawing client (dapple-ui curates ~150 lucide names); we are not the
--     party that knows what can be drawn. Stored verbatim as authored.
--   * A name we never heard of is a CLIENT-SIDE degrade, not an error: the
--     client falls back `authored icon -> kind glyph -> neutral disc`, and the
--     kind glyph is what it already draws today. So an unknown name costs
--     nothing and a client restyle is a registry edit, never a contract change.
--   * Ordered and plural. Up to five names so a stage can vary without an
--     author committing to a forever-glyph; the client picks one
--     deterministically. Authored ORDER is load-bearing (the client reads the
--     first five), so this is a JSON array, never a set.
--
-- Authored as `:::stage { icon: ["radar", "scan-search"] }` (a bare string is
-- accepted and normalized to a one-element array at parse time). Folds into the
-- stage content hash with the same empty-omit trick `kind` uses, so an
-- icon-less stage keeps its hash and no guide mass-re-versions.

-- ============================================================================
-- rev.stages — add the authored icon-name array
--   icons nullable (absent ⇒ no icons; the client draws its kind fallback).
--   NULL and '[]' are the same fact; the writer normalizes empty to NULL so the
--   hash fold and the row agree on "absent".
--   The CHECK is a SHAPE guard only (is it an array?), never a vocabulary
--   guard — validating the names is precisely what this column must not do.
--   Deliberately NO index: nothing filters or joins stages BY icon, and the
--   migration-96 index audit is explicit that a column only ever projected does
--   not earn one.
-- ============================================================================
ALTER TABLE rev.stages
    ADD COLUMN icons JSONB,
    ADD CONSTRAINT stages_icons_is_array
        CHECK (icons IS NULL OR jsonb_typeof(icons) = 'array');
