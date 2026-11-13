-- Migration 20: Remove playbook_steps/playbook_tasks tables
--
-- Migrates the associative hierarchy from:
--   playbook_stages -> playbook_steps -> playbook_tasks
-- To the simpler canonical form:
--   stage_steps, step_tasks
--
-- Also renames directive/overlay tables and migrates FK columns
-- in dispatch_steps, dispatch_tasks, and prompt_responses.

-- ============================================================================
-- 1. Add unique constraints on stage_steps and step_tasks (for ON CONFLICT)
-- ============================================================================

ALTER TABLE rev.stage_steps
  ADD CONSTRAINT stage_steps_stage_id_step_id_unique UNIQUE (stage_id, step_id);

ALTER TABLE rev.step_tasks
  ADD CONSTRAINT step_tasks_step_id_task_id_unique UNIQUE (step_id, task_id);

-- ============================================================================
-- 2. Migrate playbook_steps -> stage_steps (skip duplicates)
-- ============================================================================

INSERT INTO rev.stage_steps (stage_id, step_id)
SELECT pst.stage_id, ps.step_id
FROM rev.playbook_steps ps
JOIN rev.playbook_stages pst ON pst.id = ps.playbook_stage_id
ON CONFLICT DO NOTHING;

-- ============================================================================
-- 3. Migrate playbook_tasks -> step_tasks (skip duplicates)
-- ============================================================================

INSERT INTO rev.step_tasks (step_id, task_id)
SELECT ps.step_id, pt.task_id
FROM rev.playbook_tasks pt
JOIN rev.playbook_steps ps ON ps.id = pt.playbook_step_id
ON CONFLICT DO NOTHING;

-- ============================================================================
-- 4. Rename directive/overlay tables
-- ============================================================================

ALTER TABLE rev.playbook_step_directives RENAME TO stage_step_directives;
ALTER TABLE rev.playbook_step_overlays RENAME TO stage_step_overlays;
ALTER TABLE rev.playbook_task_overlays RENAME TO step_task_overlays;

-- ============================================================================
-- 5. Rename the index on directives if it exists
-- ============================================================================

ALTER INDEX IF EXISTS rev.playbook_step_directives_playbook_step_id_idx
  RENAME TO stage_step_directives_stage_step_id_idx;

-- ============================================================================
-- 6. Migrate directive/overlay FK columns
-- ============================================================================

-- 6a. stage_step_directives: playbook_step_id -> stage_step_id
ALTER TABLE rev.stage_step_directives ADD COLUMN stage_step_id UUID;

UPDATE rev.stage_step_directives ssd
SET stage_step_id = (
  SELECT ss.id FROM rev.stage_steps ss
  JOIN rev.playbook_stages pst ON pst.stage_id = ss.stage_id
  JOIN rev.playbook_steps ps ON ps.playbook_stage_id = pst.id
  WHERE ps.id = ssd.playbook_step_id
  LIMIT 1
);

ALTER TABLE rev.stage_step_directives
  ALTER COLUMN stage_step_id SET NOT NULL,
  ADD CONSTRAINT stage_step_directives_stage_step_id_fkey
    FOREIGN KEY (stage_step_id) REFERENCES rev.stage_steps (id),
  DROP COLUMN playbook_step_id;

-- 6b. stage_step_overlays: playbook_step_id -> stage_step_id
ALTER TABLE rev.stage_step_overlays ADD COLUMN stage_step_id UUID;

UPDATE rev.stage_step_overlays sso
SET stage_step_id = (
  SELECT ss.id FROM rev.stage_steps ss
  JOIN rev.playbook_stages pst ON pst.stage_id = ss.stage_id
  JOIN rev.playbook_steps ps ON ps.playbook_stage_id = pst.id
  WHERE ps.id = sso.playbook_step_id
  LIMIT 1
);

ALTER TABLE rev.stage_step_overlays
  ALTER COLUMN stage_step_id SET NOT NULL,
  ADD CONSTRAINT stage_step_overlays_stage_step_id_fkey
    FOREIGN KEY (stage_step_id) REFERENCES rev.stage_steps (id),
  DROP COLUMN playbook_step_id;

-- 6c. step_task_overlays: playbook_task_id -> step_task_id
ALTER TABLE rev.step_task_overlays ADD COLUMN step_task_id UUID;

UPDATE rev.step_task_overlays sto
SET step_task_id = (
  SELECT stk.id FROM rev.step_tasks stk
  JOIN rev.playbook_steps ps ON ps.step_id = stk.step_id
  JOIN rev.playbook_tasks pt ON pt.playbook_step_id = ps.id
    AND pt.task_id = stk.task_id
  WHERE pt.id = sto.playbook_task_id
  LIMIT 1
);

ALTER TABLE rev.step_task_overlays
  ALTER COLUMN step_task_id SET NOT NULL,
  ADD CONSTRAINT step_task_overlays_step_task_id_fkey
    FOREIGN KEY (step_task_id) REFERENCES rev.step_tasks (id),
  DROP COLUMN playbook_task_id;

-- ============================================================================
-- 7. Migrate dispatch_steps.playbook_step_id -> stage_step_id
-- ============================================================================

ALTER TABLE rev.dispatch_steps ADD COLUMN stage_step_id UUID;

UPDATE rev.dispatch_steps ds
SET stage_step_id = (
  SELECT ss.id FROM rev.stage_steps ss
  JOIN rev.playbook_stages pst ON pst.stage_id = ss.stage_id
  JOIN rev.playbook_steps ps ON ps.playbook_stage_id = pst.id
  WHERE ps.id = ds.playbook_step_id
  LIMIT 1
);

ALTER TABLE rev.dispatch_steps
  ALTER COLUMN stage_step_id SET NOT NULL,
  ADD CONSTRAINT dispatch_steps_stage_step_id_fkey
    FOREIGN KEY (stage_step_id) REFERENCES rev.stage_steps (id),
  DROP COLUMN playbook_step_id;

CREATE INDEX dispatch_steps_stage_step_id_idx ON rev.dispatch_steps (stage_step_id);

-- ============================================================================
-- 8. Migrate dispatch_tasks.playbook_task_id -> step_task_id
-- ============================================================================

ALTER TABLE rev.dispatch_tasks ADD COLUMN step_task_id UUID;

UPDATE rev.dispatch_tasks dt
SET step_task_id = (
  SELECT stk.id FROM rev.step_tasks stk
  JOIN rev.playbook_steps ps ON ps.step_id = stk.step_id
  JOIN rev.playbook_tasks pt ON pt.playbook_step_id = ps.id
    AND pt.task_id = stk.task_id
  WHERE pt.id = dt.playbook_task_id
  LIMIT 1
);

ALTER TABLE rev.dispatch_tasks
  ALTER COLUMN step_task_id SET NOT NULL,
  ADD CONSTRAINT dispatch_tasks_step_task_id_fkey
    FOREIGN KEY (step_task_id) REFERENCES rev.step_tasks (id),
  DROP COLUMN playbook_task_id;

CREATE INDEX dispatch_tasks_step_task_id_idx ON rev.dispatch_tasks (step_task_id);

-- ============================================================================
-- 8.5 Migrate prompt_responses.playbook_task_id -> step_task_id
--
-- This table was not in the original plan but has a FK to playbook_tasks
-- that would block the DROP TABLE in step 9.
-- ============================================================================

ALTER TABLE rev.prompt_responses ADD COLUMN step_task_id UUID;

UPDATE rev.prompt_responses pr
SET step_task_id = (
  SELECT stk.id FROM rev.step_tasks stk
  JOIN rev.playbook_steps ps ON ps.step_id = stk.step_id
  JOIN rev.playbook_tasks pt ON pt.playbook_step_id = ps.id
    AND pt.task_id = stk.task_id
  WHERE pt.id = pr.playbook_task_id
  LIMIT 1
);

ALTER TABLE rev.prompt_responses
  ALTER COLUMN step_task_id SET NOT NULL,
  ADD CONSTRAINT prompt_responses_step_task_id_fkey
    FOREIGN KEY (step_task_id) REFERENCES rev.step_tasks (id),
  DROP COLUMN playbook_task_id;

CREATE INDEX prompt_responses_step_task_id_idx ON rev.prompt_responses (step_task_id);

-- ============================================================================
-- 9. Drop old tables (order matters for FK dependencies)
-- ============================================================================

DROP TABLE IF EXISTS rev.playbook_step_engine_configs;
DROP TABLE IF EXISTS rev.playbook_tasks;
DROP TABLE IF EXISTS rev.playbook_steps;
