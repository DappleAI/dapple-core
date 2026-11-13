WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],          $2::UUID[]       ) WITH ORDINALITY AS o(id, universal_user_id, ord)
),
playbook_page AS (
    SELECT objs.id
    FROM args
    JOIN rev.playbooks objs ON objs.id = args.id
    ORDER BY objs.id
    LIMIT $3 OFFSET $4
)
SELECT
    objs.id,
    objs.versioned_object_id,
    vo.path_id,
    ppd.name,
    ppd.slug,
    vo.content_hash,
    objs.created_at,
    pst.id AS "playbook_stage_id?",
    pst.created_at AS "playbook_stage_created_at?",
    s.id AS "playbook_stage_stage_id?",
    ss.id AS "stage_step_id?",
    ss.created_at AS "stage_step_created_at?",
    st.id AS "stage_step_step_id?",
    stk.id AS "step_task_id?",
    t.id AS "step_task_task_id?",
    stk.created_at AS "step_task_created_at?"
FROM playbook_page pp
JOIN rev.playbooks objs ON objs.id = pp.id
JOIN rev.versioned_objects vo ON vo.id = objs.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.playbook_path_details ppd ON ppd.path_id = op.id
LEFT JOIN rev.playbook_stages pst ON objs.id = pst.playbook_id
LEFT JOIN rev.stages s ON pst.stage_id = s.id
LEFT JOIN rev.stage_steps ss ON s.id = ss.stage_id
LEFT JOIN rev.steps st ON ss.step_id = st.id
LEFT JOIN rev.step_tasks stk ON stk.step_id = st.id
LEFT JOIN rev.tasks t ON t.id = stk.task_id
ORDER BY objs.id, pst.ord NULLS FIRST, ss.ord NULLS FIRST, stk.ord NULLS FIRST;
