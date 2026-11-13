-- Migration 6: Playbook structure
-- playbook_stages, playbook_steps, playbook_tasks, directives, overlays, engine configs

-- ============================================================================
-- rev.playbook_stages — Associative (Playbook <-> Stage)
-- ============================================================================
CREATE TABLE rev.playbook_stages (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    playbook_id UUID NOT NULL REFERENCES rev.playbooks (id),
    stage_id UUID NOT NULL REFERENCES rev.stages (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX playbook_stages_playbook_idx ON rev.playbook_stages (playbook_id);
CREATE INDEX playbook_stages_stage_idx ON rev.playbook_stages (stage_id);

-- ============================================================================
-- rev.playbook_steps — Associative (PlaybookStage <-> Step)
-- ============================================================================
CREATE TABLE rev.playbook_steps (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    playbook_stage_id UUID NOT NULL REFERENCES rev.playbook_stages (id),
    step_id UUID NOT NULL REFERENCES rev.steps (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX playbook_steps_playbook_stage_idx ON rev.playbook_steps (playbook_stage_id);
CREATE INDEX playbook_steps_step_idx ON rev.playbook_steps (step_id);

-- ============================================================================
-- rev.playbook_tasks — Associative (PlaybookStep <-> Task)
-- ============================================================================
CREATE TABLE rev.playbook_tasks (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    playbook_step_id UUID NOT NULL REFERENCES rev.playbook_steps (id),
    task_id UUID NOT NULL REFERENCES rev.tasks (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (playbook_step_id, task_id)
);

CREATE INDEX playbook_tasks_playbook_step_idx ON rev.playbook_tasks (playbook_step_id);
CREATE INDEX playbook_tasks_task_idx ON rev.playbook_tasks (task_id);

-- ============================================================================
-- rev.directives — Versioned conditional logic
-- ============================================================================
CREATE TABLE rev.directives (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    logic JSONB NOT NULL,
    max_attempts INT NOT NULL DEFAULT 1 CHECK (max_attempts > 0),
    retry_delay_ms INT CHECK (retry_delay_ms IS NULL OR retry_delay_ms >= 0),
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX directives_versioned_object_idx ON rev.directives (versioned_object_id);

-- ============================================================================
-- rev.playbook_stage_directives — Directive for stage (0..1)
-- ============================================================================
CREATE TABLE rev.playbook_stage_directives (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    playbook_stage_id UUID NOT NULL UNIQUE REFERENCES rev.playbook_stages (id),
    directive_id UUID NOT NULL REFERENCES rev.directives (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX playbook_stage_directives_stage_idx ON rev.playbook_stage_directives (playbook_stage_id);
CREATE INDEX playbook_stage_directives_directive_idx ON rev.playbook_stage_directives (directive_id);

-- ============================================================================
-- rev.playbook_step_directives — Directive for step (0..1)
-- ============================================================================
CREATE TABLE rev.playbook_step_directives (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    playbook_step_id UUID NOT NULL UNIQUE REFERENCES rev.playbook_steps (id),
    directive_id UUID NOT NULL REFERENCES rev.directives (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX playbook_step_directives_step_idx ON rev.playbook_step_directives (playbook_step_id);
CREATE INDEX playbook_step_directives_directive_idx ON rev.playbook_step_directives (directive_id);

-- ============================================================================
-- rev.playbook_stage_overlays — Visibility control per stage
-- ============================================================================
CREATE TABLE rev.playbook_stage_overlays (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    playbook_stage_id UUID NOT NULL REFERENCES rev.playbook_stages (id),
    value_slot_path_id UUID NOT NULL REFERENCES rev.value_slot_path_details (path_id),
    visibility TEXT NOT NULL CHECK (visibility IN ('expose', 'mask')),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (playbook_stage_id, value_slot_path_id)
);

CREATE INDEX playbook_stage_overlays_stage_idx ON rev.playbook_stage_overlays (playbook_stage_id);
CREATE INDEX playbook_stage_overlays_value_slot_path_idx ON rev.playbook_stage_overlays (value_slot_path_id);

-- ============================================================================
-- rev.playbook_step_overlays — Visibility control per step
-- ============================================================================
CREATE TABLE rev.playbook_step_overlays (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    playbook_step_id UUID NOT NULL REFERENCES rev.playbook_steps (id),
    value_slot_path_id UUID NOT NULL REFERENCES rev.value_slot_path_details (path_id),
    visibility TEXT NOT NULL CHECK (visibility IN ('expose', 'mask')),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (playbook_step_id, value_slot_path_id)
);

CREATE INDEX playbook_step_overlays_step_idx ON rev.playbook_step_overlays (playbook_step_id);
CREATE INDEX playbook_step_overlays_value_slot_path_idx ON rev.playbook_step_overlays (value_slot_path_id);

-- ============================================================================
-- rev.playbook_task_overlays — Visibility control per task
-- ============================================================================
CREATE TABLE rev.playbook_task_overlays (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    playbook_task_id UUID NOT NULL REFERENCES rev.playbook_tasks (id),
    value_slot_path_id UUID NOT NULL REFERENCES rev.value_slot_path_details (path_id),
    visibility TEXT NOT NULL CHECK (visibility IN ('expose', 'mask')),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (playbook_task_id, value_slot_path_id)
);

CREATE INDEX playbook_task_overlays_task_idx ON rev.playbook_task_overlays (playbook_task_id);
CREATE INDEX playbook_task_overlays_value_slot_path_idx ON rev.playbook_task_overlays (value_slot_path_id);

-- ============================================================================
-- rev.playbook_step_engine_configs — Links playbook steps to LLM engines
-- ============================================================================
CREATE TABLE rev.playbook_step_engine_configs (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    playbook_step_id UUID NOT NULL REFERENCES rev.playbook_steps (id),
    llm_engine_id UUID NOT NULL REFERENCES rev.llm_engines (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (playbook_step_id)
);

CREATE INDEX playbook_step_engine_configs_step_idx ON rev.playbook_step_engine_configs (playbook_step_id);
CREATE INDEX playbook_step_engine_configs_engine_idx ON rev.playbook_step_engine_configs (llm_engine_id);
