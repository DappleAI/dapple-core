-- Migration 47: Restore composite UNIQUE constraints on the renamed overlay
-- tables.
--
-- Migration 6 created playbook_step_overlays / playbook_task_overlays each with
-- UNIQUE (parent_id, value_slot_path_id). Migration 20 renamed them to
-- stage_step_overlays / step_task_overlays during the playbook-step
-- restructure and dropped those composite uniques (only playbook_stage_overlays
-- kept its constraint).
--
-- The composite unique is load-bearing for two reasons:
--   1. Overlay materialization during guide ingest is append-only and relies on
--      `INSERT ... ON CONFLICT (parent_id, value_slot_path_id) DO NOTHING` for
--      idempotency when a playbook version is reused (overlays are folded into
--      the stage/step content hash, so a reused version carries identical
--      overlays). ON CONFLICT requires a matching unique constraint.
--   2. It prevents duplicate (parent, slot) overlay rows, which would make the
--      visibility cascade ambiguous.
--
-- Safe to add unconditionally: all three overlay tables are empty (overlays
-- were never populated before the authoring pipeline landed).

ALTER TABLE rev.stage_step_overlays
    ADD CONSTRAINT stage_step_overlays_stage_step_id_value_slot_path_id_key
    UNIQUE (stage_step_id, value_slot_path_id);

ALTER TABLE rev.step_task_overlays
    ADD CONSTRAINT step_task_overlays_step_task_id_value_slot_path_id_key
    UNIQUE (step_task_id, value_slot_path_id);
