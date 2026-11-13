UPDATE rev.background_jobs
SET status = CASE WHEN attempts >= max_attempts THEN 'failed' ELSE 'queued' END,
    error_message = $2,
    scheduled_at = CASE
        WHEN attempts >= max_attempts THEN scheduled_at
        ELSE now() + ((2 ^ attempts) || ' minutes')::INTERVAL
    END,
    claimed_at = NULL
WHERE id = $1
RETURNING
    id,
    job_kind,
    target_id,
    payload,
    status,
    attempts,
    max_attempts,
    error_message,
    scheduled_at,
    claimed_at,
    completed_at,
    created_at;
