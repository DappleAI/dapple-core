UPDATE rev.background_jobs
SET status = 'running',
    claimed_at = now(),
    attempts = attempts + 1
WHERE id = (
    SELECT id
    FROM rev.background_jobs
    WHERE status = 'queued'
      AND scheduled_at <= now()
      AND (
          job_kind = ANY($1::TEXT[])
          OR $1::TEXT[] IS NULL
          OR array_length($1::TEXT[], 1) IS NULL
      )
    ORDER BY scheduled_at
    FOR UPDATE SKIP LOCKED
    LIMIT 1
)
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
