
SELECT
    dsta.context_data,
    dsta.context_metadata,
    dsta.task_id
FROM rev.dispatch_task_attempts dsta
JOIN rev.tasks t ON t.id = dsta.task_id
WHERE dsta.id = $1;
