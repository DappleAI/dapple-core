WITH latest AS (
    SELECT DISTINCT ON (ev.attempt_id)
        ev.attempt_id,
        ev.phase_code,
        ev.context_metadata->>'executor' AS executor
    FROM rev.dispatch_task_attempt_events ev
    ORDER BY ev.attempt_id, ev.id DESC
)
SELECT l.attempt_id
FROM latest l
JOIN rev.dispatch_phases dp ON dp.code = l.phase_code
WHERE NOT dp.terminal
  AND (l.executor = $1::TEXT OR l.executor IS NULL);
