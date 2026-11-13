-- Migration 7: Journey execution
-- journeys, dispatches, dispatch stages/steps/tasks, attempts, events

-- ============================================================================
-- rev.journeys — User's session/instance of a playbook
-- ============================================================================
CREATE TABLE rev.journeys (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    playbook_id UUID NOT NULL REFERENCES rev.playbooks (id),
    user_id BIGINT NOT NULL REFERENCES rev.user_details (user_id),
    name TEXT NOT NULL,
    -- brief_pack_id added in migration 8 (after brief_packs table exists)
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX journeys_playbook_idx ON rev.journeys (playbook_id);
CREATE INDEX journeys_user_idx ON rev.journeys (user_id);

-- ============================================================================
-- web.user_favorite_journeys — Append-only with removal tracking
-- ============================================================================
CREATE TABLE web.user_favorite_journeys (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES rev.user_details (user_id),
    journey_id UUID NOT NULL REFERENCES rev.journeys (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX user_favorite_journeys_lookup ON web.user_favorite_journeys (user_id, journey_id, id DESC);

CREATE TABLE web.user_favorite_journeys_removed (
    user_favorite_journeys_id UUID PRIMARY KEY REFERENCES web.user_favorite_journeys (id),
    removed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================================
-- rev.dispatches — Execution instance tied to journey
-- ============================================================================
CREATE TABLE rev.dispatches (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    journey_id UUID NOT NULL REFERENCES rev.journeys (id),
    user_id BIGINT NOT NULL REFERENCES rev.user_details (user_id),
    intent JSONB NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX dispatches_journey_idx ON rev.dispatches (journey_id);
CREATE INDEX dispatches_user_idx ON rev.dispatches (user_id);

-- ============================================================================
-- rev.dispatch_stages — Dispatch → PlaybookStage execution
-- ============================================================================
CREATE TABLE rev.dispatch_stages (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_id UUID NOT NULL REFERENCES rev.dispatches (id),
    playbook_stage_id UUID NOT NULL REFERENCES rev.playbook_stages (id),
    params JSONB NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (dispatch_id, playbook_stage_id)
);

CREATE INDEX dispatch_stages_dispatch_idx ON rev.dispatch_stages (dispatch_id);
CREATE INDEX dispatch_stages_playbook_stage_idx ON rev.dispatch_stages (playbook_stage_id);

-- ============================================================================
-- rev.dispatch_steps — Dispatch → PlaybookStep execution
-- ============================================================================
CREATE TABLE rev.dispatch_steps (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_stage_id UUID NOT NULL REFERENCES rev.dispatch_stages (id),
    playbook_step_id UUID NOT NULL REFERENCES rev.playbook_steps (id),
    params JSONB NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (dispatch_stage_id, playbook_step_id)
);

CREATE INDEX dispatch_steps_dispatch_stage_idx ON rev.dispatch_steps (dispatch_stage_id);
CREATE INDEX dispatch_steps_playbook_step_idx ON rev.dispatch_steps (playbook_step_id);

-- ============================================================================
-- rev.dispatch_tasks — Dispatch → PlaybookTask execution
-- ============================================================================
CREATE TABLE rev.dispatch_tasks (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_step_id UUID NOT NULL REFERENCES rev.dispatch_steps (id),
    playbook_task_id UUID NOT NULL REFERENCES rev.playbook_tasks (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (dispatch_step_id, playbook_task_id)
);

CREATE INDEX dispatch_tasks_dispatch_step_idx ON rev.dispatch_tasks (dispatch_step_id);
CREATE INDEX dispatch_tasks_playbook_task_idx ON rev.dispatch_tasks (playbook_task_id);

-- ============================================================================
-- rev.dispatch_task_attempts — Task execution attempts
-- ============================================================================
CREATE TABLE rev.dispatch_task_attempts (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_step_id UUID NOT NULL REFERENCES rev.dispatch_steps (id),
    task_id UUID NOT NULL REFERENCES rev.tasks (id),
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'incomplete', 'canceled')),
    context_data JSONB DEFAULT '{}'::jsonb NOT NULL,
    context_metadata JSONB DEFAULT '{}'::jsonb NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX dispatch_task_attempts_dispatch_step_idx ON rev.dispatch_task_attempts (dispatch_step_id);
CREATE INDEX dispatch_task_attempts_task_idx ON rev.dispatch_task_attempts (task_id);

-- ============================================================================
-- rev.dispatch_task_attempt_events — Phase transition events
-- ============================================================================
CREATE TABLE rev.dispatch_task_attempt_events (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    attempt_id UUID NOT NULL REFERENCES rev.dispatch_task_attempts (id),
    phase_code TEXT NOT NULL REFERENCES rev.dispatch_phases (code),
    context_data JSONB,
    context_metadata JSONB,
    context_temp_data JSONB,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX dispatch_task_attempt_events_attempt_idx ON rev.dispatch_task_attempt_events (attempt_id);
CREATE INDEX dispatch_task_attempt_events_phase_idx ON rev.dispatch_task_attempt_events (phase_code);
