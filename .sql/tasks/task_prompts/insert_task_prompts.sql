-- Insert task_prompts linking tasks to prompts
-- Parameters:
--   $1: task_ids (UUID[]) - task IDs
--   $2: prompt_ids (UUID[]) - prompt IDs
-- Note: prompt_name is derived from prompt_paths.name via join, not passed as parameter

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
-- Resolve IDs and get prompt name via path details
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
    RETURNING id
)
SELECT
    tp.id,
    tp.task_id,
    tp.prompt_id,
    r.prompt_name,
    tp.created_at
FROM resolved r
JOIN rev.task_prompts tp ON tp.task_id = r.task_id AND tp.prompt_id = r.prompt_id
ORDER BY tp.id;
