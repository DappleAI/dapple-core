-- Insert step_tasks (associative table entries)
-- Uses ON CONFLICT DO NOTHING to skip duplicates when patched steps
-- already have an existing task link.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[]
    ) WITH ORDINALITY
    AS o(
        step_id,
        task_id,
        ord
    )
),
-- Resolve IDs with ordinality preserved
resolved AS (
    SELECT
        args.ord,
        s.id AS step_id,
        t.id AS task_id
    FROM args
    JOIN rev.steps s ON args.step_id = s.id
    JOIN rev.tasks t ON args.task_id = t.id
),
_inserted AS (
    INSERT INTO rev.step_tasks (step_id, task_id)
    SELECT r.step_id, r.task_id
    FROM resolved r
    ORDER BY r.ord
    ON CONFLICT (step_id, task_id) DO NOTHING
    RETURNING id
)
SELECT
    st.id,
    st.step_id,
    st.task_id,
    st.created_at
FROM resolved r
JOIN rev.step_tasks st ON st.step_id = r.step_id AND st.task_id = r.task_id
ORDER BY st.id;