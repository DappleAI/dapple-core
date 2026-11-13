WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::UUID[],
        $3::JSONB[]
    ) WITH ORDINALITY AS o(job_kind, target_id, payload, ord)
),
objs AS (
    INSERT INTO rev.background_jobs (job_kind, target_id, payload)
    SELECT args.job_kind, args.target_id, args.payload
    FROM args
    ORDER BY args.ord
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
        created_at
)
SELECT
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
    created_at
FROM objs
ORDER BY id;
