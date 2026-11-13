-- Select task_prompts by IDs
-- Parameters:
--   $1: ids (UUID[])
-- Returns prompt_name derived from prompt_paths.name via join

WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    tp.id,
    tp.task_id,
    tp.prompt_id,
    ppd.name AS prompt_name,
    tp.created_at
FROM args
JOIN rev.task_prompts tp ON tp.id = args.id
JOIN rev.tasks t ON t.id = tp.task_id
JOIN rev.prompts p ON p.id = tp.prompt_id
JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.prompt_path_details ppd ON ppd.path_id = op.id
ORDER BY tp.id;
