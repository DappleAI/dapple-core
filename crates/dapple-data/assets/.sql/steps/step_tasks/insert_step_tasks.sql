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
resolved AS (
    SELECT
        args.ord,
        ROW_NUMBER() OVER (PARTITION BY s.id ORDER BY args.ord) AS pos,
        s.id AS step_id,
        t.id AS task_id
    FROM args
    JOIN rev.steps s ON args.step_id = s.id
    JOIN rev.tasks t ON args.task_id = t.id
),
inserted AS (
    INSERT INTO rev.step_tasks (step_id, task_id, ord)
    SELECT r.step_id, r.task_id, r.pos
    FROM resolved r
    ORDER BY r.ord
    ON CONFLICT (step_id, task_id) DO NOTHING
    RETURNING id, step_id, task_id, ord, created_at
),
all_links AS (
    SELECT id, step_id, task_id, ord, created_at
    FROM inserted
    UNION ALL
    SELECT st.id, st.step_id, st.task_id, st.ord, st.created_at
    FROM resolved r
    JOIN rev.step_tasks st ON st.step_id = r.step_id AND st.task_id = r.task_id
    WHERE NOT EXISTS (
        SELECT 1 FROM inserted i
        WHERE i.step_id = r.step_id AND i.task_id = r.task_id
    )
)
SELECT
    al.id AS "id!",
    al.step_id AS "step_id!",
    al.task_id AS "task_id!",
    al.created_at AS "created_at!"
FROM all_links al
ORDER BY al.step_id, al.ord;