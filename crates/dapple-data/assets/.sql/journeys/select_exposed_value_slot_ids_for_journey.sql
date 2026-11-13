
WITH journey_stages AS (
    SELECT ps.id AS playbook_stage_id, ps.stage_id
    FROM rev.journeys j
    JOIN rev.playbook_stages ps ON ps.playbook_id = j.playbook_id
    WHERE j.id = $1
),
stage_tasks AS (
    SELECT js.playbook_stage_id, ss.id AS stage_step_id, stk.id AS step_task_id
    FROM journey_stages js
    JOIN rev.stage_steps ss ON ss.stage_id = js.stage_id
    JOIN rev.step_tasks stk ON stk.step_id = ss.step_id
),
all_overlays AS (
    SELECT st.step_task_id, o.value_slot_path_id, o.visibility, 1 AS priority
    FROM rev.playbook_stage_overlays o
    JOIN stage_tasks st ON st.playbook_stage_id = o.playbook_stage_id
    UNION ALL
    SELECT st.step_task_id, o.value_slot_path_id, o.visibility, 2 AS priority
    FROM rev.stage_step_overlays o
    JOIN stage_tasks st ON st.stage_step_id = o.stage_step_id
    UNION ALL
    SELECT st.step_task_id, o.value_slot_path_id, o.visibility, 3 AS priority
    FROM rev.step_task_overlays o
    JOIN stage_tasks st ON st.step_task_id = o.step_task_id
),
ranked AS (
    SELECT DISTINCT ON (step_task_id, value_slot_path_id)
        value_slot_path_id, visibility
    FROM all_overlays
    ORDER BY step_task_id, value_slot_path_id, priority DESC
),
exposed_paths AS (
    SELECT DISTINCT value_slot_path_id
    FROM ranked
    WHERE visibility = 'expose'
)
SELECT DISTINCT vs.id AS "id!"
FROM exposed_paths ep
JOIN rev.versioned_objects vo ON vo.path_id = ep.value_slot_path_id
JOIN rev.value_slots vs ON vs.versioned_object_id = vo.id;
