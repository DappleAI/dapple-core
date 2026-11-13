-- Select all task_prompts for a step_task by its ID
-- Used by Nu commands to build prompt_mappings (name -> id)
-- Parameters:
--   $1: step_task_id (UUID) - step_task ID

SELECT
    tp.id,
    tp.task_id,
    tp.prompt_id,
    ppd.name AS prompt_name,
    tp.created_at
FROM rev.step_tasks stk
JOIN rev.tasks t ON t.id = stk.task_id
JOIN rev.task_prompts tp ON tp.task_id = t.id
JOIN rev.prompts p ON p.id = tp.prompt_id
JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.prompt_path_details ppd ON ppd.path_id = op.id
WHERE stk.id = $1
ORDER BY tp.id;
