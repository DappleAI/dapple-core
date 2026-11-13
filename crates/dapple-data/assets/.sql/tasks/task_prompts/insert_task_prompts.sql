
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[]
    ) WITH ORDINALITY
    AS o(
        task_id,
        prompt_id,
        ord
    )
),
resolved AS (
    SELECT
        t.id AS task_id,
        p.id AS prompt_id,
        ppd.name AS prompt_name,
        args.ord
    FROM args
    JOIN rev.tasks t ON args.task_id = t.id
    JOIN rev.prompts p ON args.prompt_id = p.id
    JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.prompt_path_details ppd ON ppd.path_id = op.id
),
_inserted AS (
    INSERT INTO rev.task_prompts (task_id, prompt_id)
    SELECT resolved.task_id, resolved.prompt_id
    FROM resolved
    ORDER BY resolved.ord
    ON CONFLICT (task_id, prompt_id) DO NOTHING
    RETURNING id, task_id, prompt_id, created_at
),
all_links AS (
    SELECT id, task_id, prompt_id, created_at FROM _inserted
    UNION ALL
    SELECT tp.id, tp.task_id, tp.prompt_id, tp.created_at
    FROM resolved r
    JOIN rev.task_prompts tp ON tp.task_id = r.task_id AND tp.prompt_id = r.prompt_id
    WHERE NOT EXISTS (
        SELECT 1 FROM _inserted i
        WHERE i.task_id = r.task_id AND i.prompt_id = r.prompt_id
    )
)
SELECT
    al.id AS "id!",
    al.task_id AS "task_id!",
    al.prompt_id AS "prompt_id!",
    r.prompt_name AS "prompt_name!",
    al.created_at AS "created_at!"
FROM all_links al
JOIN resolved r ON r.task_id = al.task_id AND r.prompt_id = al.prompt_id
ORDER BY al.task_id, al.id;
