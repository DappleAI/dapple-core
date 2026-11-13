-- Migration 97: Typed kind for stages
--
-- A stage reaches journey clients as a UUID plus done/blocked booleans; nothing
-- on the wire says what a stage IS, so every client that draws a stage invented
-- its identity locally (dapple-ui indexed icons/titles by stage ordinal against
-- 5-element arrays, which broke visibly at mark-the-bdc's 15 stages — see
-- docs/plans/dapple-ui-stage-kind-and-title.md). This adds a closed, authored
-- stage-kind vocabulary, threaded exactly like the brief-output display-hint
-- precedent (migration 55): a seeded lookup table with a nullable FK column on
-- rev.stages. The key is authored on the :::stage directive (`kind: "fetch"`),
-- folds into the stage content hash (empty-omit, so kindless stages keep their
-- hashes), and rides the journey /steps and /stages reads as `kind`.

-- ============================================================================
-- rev.stage_kinds — closed pane vocabulary (semantic, not pictorial)
--   intake   the operator supplies the values the work starts from
--   fetch    retrieves source data; runs by itself
--   analyze  derives a reading from data already held; runs by itself
--   decide   the operator chooses among generated candidates
--   review   the operator reads and confirms; produces no new data
--   compare  weighs two already-produced things against each other
--   assemble builds the deliverable from everything upstream
--   deliver  the finished artifact; the terminal pane
-- ============================================================================
CREATE TABLE rev.stage_kinds (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.stage_kinds (key) VALUES
    ('intake'), ('fetch'), ('analyze'), ('decide'),
    ('review'), ('compare'), ('assemble'), ('deliver');

-- ============================================================================
-- rev.stages — add the kind reference
--   kind_id nullable (absent ⇒ no kind; clients apply their own default).
--   Deliberately NO index on kind_id: per the migration-96 index audit, reads
--   join rev.stage_kinds by its PK to resolve the key, and no query filters or
--   joins stages BY kind_id.
-- ============================================================================
ALTER TABLE rev.stages
    ADD COLUMN kind_id SMALLINT REFERENCES rev.stage_kinds (id);
