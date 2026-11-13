-- Select a single task_prompt by task ID and prompt path name
-- Parameters:
--   $1: task_id (UUID) - task ID
--   $2: prompt_name (TEXT) - prompt path name (from prompt_paths.name)

SELECT
    tp.id,
    tp.task_id,
    tp.prompt_id,
    ppd.name AS prompt_name,
    tp.created_at
FROM rev.task_prompts tp
JOIN rev.tasks t ON t.id = tp.task_id
JOIN rev.prompts p ON p.id = tp.prompt_id
JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.prompt_path_details ppd ON ppd.path_id = op.id
WHERE tp.task_id = $1 AND ppd.name = $2;
