WITH task_context AS (
    SELECT
        stk.id AS step_task_id,
        ss.id AS stage_step_id,
        pstage.id AS playbook_stage_id
    FROM rev.step_tasks stk
    JOIN rev.stage_steps ss ON ss.step_id = stk.step_id
    JOIN rev.playbook_stages pstage ON pstage.stage_id = ss.stage_id
    WHERE stk.id = $1::UUID
),
all_overlays AS (
        SELECT
        op.id,
        vspd.name,
        o.visibility,
        1 AS priority
    FROM rev.playbook_stage_overlays o
    JOIN task_context tc ON tc.playbook_stage_id = o.playbook_stage_id
    JOIN rev.object_paths op ON op.id = o.value_slot_path_id
    JOIN rev.value_slot_path_details vspd ON vspd.path_id = op.id

    UNION ALL

        SELECT
        op.id,
        vspd.name,
        o.visibility,
        2 AS priority
    FROM rev.stage_step_overlays o
    JOIN task_context tc ON tc.stage_step_id = o.stage_step_id
    JOIN rev.object_paths op ON op.id = o.value_slot_path_id
    JOIN rev.value_slot_path_details vspd ON vspd.path_id = op.id

    UNION ALL

        SELECT
        op.id,
        vspd.name,
        o.visibility,
        3 AS priority
    FROM rev.step_task_overlays o
    JOIN task_context tc ON tc.step_task_id = o.step_task_id
    JOIN rev.object_paths op ON op.id = o.value_slot_path_id
    JOIN rev.value_slot_path_details vspd ON vspd.path_id = op.id
)
SELECT DISTINCT ON (id)
    id AS "value_slot_path_id!",
    name AS "value_slot_name!",
    visibility AS "visibility!"
FROM all_overlays
ORDER BY id, priority DESC;
