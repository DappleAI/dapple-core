-- Migration 28: Background jobs queue
-- Generic job queue with a job_kind discriminator. Extraction jobs are the
-- first consumer; embedding jobs are chained after extraction completes.
-- Claimed with SELECT FOR UPDATE SKIP LOCKED.
--
-- Statuses:
--   queued   — waiting for a worker
--   running  — claimed by a worker
--   ok       — completed successfully
--   failed   — exceeded max_attempts or permanent failure
--
-- job_kind values (informal; enforced by writer):
--   extract_pdf
--   extract_docx
--   embed_extraction

CREATE TABLE rev.background_jobs (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    job_kind TEXT NOT NULL,
    target_id UUID NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    status TEXT NOT NULL DEFAULT 'queued',
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 3,
    error_message TEXT,
    scheduled_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    claimed_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT background_jobs_status_check
        CHECK (status IN ('queued', 'running', 'ok', 'failed'))
);

-- Claim index — workers poll with:
--   SELECT ... WHERE status = 'queued' AND scheduled_at <= now()
--   ORDER BY scheduled_at FOR UPDATE SKIP LOCKED LIMIT 1
CREATE INDEX background_jobs_claim_idx
    ON rev.background_jobs (scheduled_at)
    WHERE status = 'queued';

CREATE INDEX background_jobs_kind_status_idx
    ON rev.background_jobs (job_kind, status);
CREATE INDEX background_jobs_target_idx
    ON rev.background_jobs (target_id);
