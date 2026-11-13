-- Migration 51: Add the composite UNIQUE constraint on rev.playbook_stages.
--
-- playbook_stages (playbook_id, stage_id) is the associative link from a
-- playbook version to its stage versions. Migrations 4/6 created stage_steps,
-- step_tasks, and playbook_stages with only single-column indexes; migration 20
-- back-filled the composite UNIQUE on stage_steps and step_tasks (so their
-- ingest-time `ON CONFLICT (parent, child) DO NOTHING` is meaningful), but
-- playbook_stages was missed. Without it a re-ingest can append a duplicate
-- (playbook_id, stage_id) row that is invisible to hash_playbook — the parent
-- hash is computed from authored children, not persisted links (the Class C
-- blindspot in docs/plans/ingest-backfill-systemic-sweep.md).
--
-- Adding the constraint (a) makes the matching ingest-time
-- `ON CONFLICT (playbook_id, stage_id) DO NOTHING` legal so the caller-side
-- completeness guard in assemble_playbook_children_in_tx can fire, and (b)
-- makes duplicate stage links structurally impossible.
--
-- Safe to add unconditionally (mirrors migration 47): playbook_stages is empty
-- at migration time on fresh DBs, and the live dev DB has zero duplicate
-- (playbook_id, stage_id) groups today.

ALTER TABLE rev.playbook_stages
    ADD CONSTRAINT playbook_stages_playbook_id_stage_id_unique
    UNIQUE (playbook_id, stage_id);
