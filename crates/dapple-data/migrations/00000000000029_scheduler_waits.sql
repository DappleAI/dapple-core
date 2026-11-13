-- Migration 29: Scheduler wait framework tables
-- Schema for the generic task-wait framework. Tables are landed now so the
-- scheduler refactor (Phase C) can slot in without a migration dependency,
-- even though the first E2E cut uses inline polling in the asset text command
-- rather than the full scheduler.
--
-- wait_kind values (first supported: 'extraction'; future: 'human_input',
-- 'llm_callback', 'sub_dispatch'):

-- ============================================================================
-- rev.dispatch_task_required_extractions — AST-derived requirements
-- Populated at dispatch creation time by the scheduler's AST scan. Each row
-- declares that a task needs a specific asset_id extracted with a given
-- (extractor_kind, extractor_version) before it can run.
-- ============================================================================
CREATE TABLE rev.dispatch_task_required_extractions (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_task_id UUID NOT NULL REFERENCES rev.dispatch_tasks (id) ON DELETE CASCADE,
    asset_id UUID NOT NULL REFERENCES rev.assets (id),
    extractor_kind TEXT NOT NULL,
    extractor_version INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (dispatch_task_id, asset_id, extractor_kind, extractor_version)
);

CREATE INDEX dispatch_task_required_extractions_task_idx
    ON rev.dispatch_task_required_extractions (dispatch_task_id);
CREATE INDEX dispatch_task_required_extractions_asset_idx
    ON rev.dispatch_task_required_extractions (asset_id);

-- ============================================================================
-- rev.dispatch_task_waits — Runtime wait state
-- Generic wait rows; wait_kind discriminates the resolver. For extraction
-- waits, wait_target_id points to rev.asset_extractions.id once it exists,
-- or to rev.assets.id prior to extraction kickoff.
-- ============================================================================
CREATE TABLE rev.dispatch_task_waits (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_task_id UUID NOT NULL REFERENCES rev.dispatch_tasks (id) ON DELETE CASCADE,
    wait_kind TEXT NOT NULL,
    wait_target_id UUID NOT NULL,
    status TEXT NOT NULL DEFAULT 'waiting',
    timeout_at TIMESTAMPTZ,
    resolved_at TIMESTAMPTZ,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT dispatch_task_waits_status_check
        CHECK (status IN ('waiting', 'resolved', 'timeout', 'failed'))
);

CREATE INDEX dispatch_task_waits_task_idx
    ON rev.dispatch_task_waits (dispatch_task_id);
CREATE INDEX dispatch_task_waits_kind_target_idx
    ON rev.dispatch_task_waits (wait_kind, wait_target_id);
CREATE INDEX dispatch_task_waits_waiting_idx
    ON rev.dispatch_task_waits (wait_kind, wait_target_id)
    WHERE status = 'waiting';
CREATE INDEX dispatch_task_waits_timeout_idx
    ON rev.dispatch_task_waits (timeout_at)
    WHERE status = 'waiting' AND timeout_at IS NOT NULL;
