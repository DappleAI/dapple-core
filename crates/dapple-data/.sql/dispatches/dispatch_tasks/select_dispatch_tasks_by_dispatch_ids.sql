SELECT
    dt.id,
    ds.id AS dispatch_step_id,
    stk.id AS step_task_id,
    t.id AS task_id,
    dt.created_at,
    dta.status
FROM rev.dispatch_tasks dt
JOIN rev.dispatch_steps ds ON dt.dispatch_step_id = ds.id
JOIN rev.dispatch_stages dst ON ds.dispatch_stage_id = dst.id
JOIN rev.dispatches d ON dst.dispatch_id = d.id
JOIN rev.step_tasks stk ON dt.step_task_id = stk.id
JOIN rev.tasks t ON stk.task_id = t.id
LEFT JOIN LATERAL (
    SELECT COALESCE(ev.phase_code, a.status) as status
    FROM rev.dispatch_task_attempts a
    LEFT JOIN LATERAL (
        SELECT phase_code
        FROM rev.dispatch_task_attempt_events ev2
        WHERE ev2.attempt_id = a.id
        ORDER BY ev2.created_at DESC, ev2.id DESC
        LIMIT 1
    ) ev ON true
    WHERE a.dispatch_step_id = ds.id
      AND a.task_id = t.id
    ORDER BY a.created_at DESC
    LIMIT 1
) dta ON true
WHERE d.id = $1::UUID;
