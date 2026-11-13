-- Migration 53: stored authored ordinals on the three playbook-hierarchy link tables.
--
-- Reverses the "no stored ordinals" data-layer principle for these three tables
-- ONLY (playbook_stages, stage_steps, step_tasks). Authored order previously
-- rode on UUIDv7 id magnitude; Model B repair-in-place (?backfill=true) re-INSERTs
-- a restored link with a FRESH (newest) id via ON CONFLICT (parent, child)
-- DO NOTHING, so every id-ordered consumer sorted the restored row to the END of
-- its group. A hierarchical (parent_id, child_id) sort only fixes the
-- last-of-group case; a restored FIRST/MIDDLE child still sorts after its later
-- siblings, because within the group the id magnitude no longer reflects authored
-- position. `ord` carries authored position independent of id mint time.
--
-- ord is a PER-PARENT position (1..k within each parent), so it is stable under a
-- partial-parent repair batch and the per-parent UNIQUE holds. The backfill
-- derives authored order from current id-magnitude per parent -- the best signal
-- available in the DB; per-parent partitioning already fixes every cross-group
-- scramble, and the only residual it cannot detect is a pre-existing WITHIN-group
-- middle-scramble in legacy data (which heals on its next genuine re-version).
--
-- The dispatch hierarchy (dispatch_stages / dispatch_steps / dispatch_tasks)
-- deliberately does NOT get an ord column: those rows are built once per dispatch
-- (never repaired-in-place) by reading this now-ord-ordered playbook hierarchy,
-- so their id-order is authored-order transitively.

-- rev.playbook_stages: stages within a playbook
ALTER TABLE rev.playbook_stages ADD COLUMN ord INTEGER;
UPDATE rev.playbook_stages ps
SET ord = sub.rn
FROM (
    SELECT id, ROW_NUMBER() OVER (PARTITION BY playbook_id ORDER BY id) AS rn
    FROM rev.playbook_stages
) sub
WHERE sub.id = ps.id;
ALTER TABLE rev.playbook_stages ALTER COLUMN ord SET NOT NULL;
ALTER TABLE rev.playbook_stages
    ADD CONSTRAINT playbook_stages_playbook_id_ord_unique UNIQUE (playbook_id, ord);

-- rev.stage_steps: steps within a stage
ALTER TABLE rev.stage_steps ADD COLUMN ord INTEGER;
UPDATE rev.stage_steps ss
SET ord = sub.rn
FROM (
    SELECT id, ROW_NUMBER() OVER (PARTITION BY stage_id ORDER BY id) AS rn
    FROM rev.stage_steps
) sub
WHERE sub.id = ss.id;
ALTER TABLE rev.stage_steps ALTER COLUMN ord SET NOT NULL;
ALTER TABLE rev.stage_steps
    ADD CONSTRAINT stage_steps_stage_id_ord_unique UNIQUE (stage_id, ord);

-- rev.step_tasks: tasks within a step
ALTER TABLE rev.step_tasks ADD COLUMN ord INTEGER;
UPDATE rev.step_tasks st
SET ord = sub.rn
FROM (
    SELECT id, ROW_NUMBER() OVER (PARTITION BY step_id ORDER BY id) AS rn
    FROM rev.step_tasks
) sub
WHERE sub.id = st.id;
ALTER TABLE rev.step_tasks ALTER COLUMN ord SET NOT NULL;
ALTER TABLE rev.step_tasks
    ADD CONSTRAINT step_tasks_step_id_ord_unique UNIQUE (step_id, ord);
