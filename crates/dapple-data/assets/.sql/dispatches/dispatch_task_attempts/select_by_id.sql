SELECT
    a.id,
    a.dispatch_step_id,
    a.task_id,
    COALESCE(
        (
            SELECT ev.phase_code
            FROM rev.dispatch_task_attempt_events ev
            WHERE ev.attempt_id = a.id
            ORDER BY ev.created_at DESC, ev.id DESC
            LIMIT 1
        ),
        'queued'
    ) AS "status!",
    a.context_data,
    a.context_metadata,
    NULL::text AS error_detail,
    a.created_at
FROM rev.dispatch_task_attempts a
WHERE a.id = $1::uuid;
