WITH input AS (
    SELECT pc.playbook_id,
           array_agg(pc.category_id ORDER BY pc.id) AS category_ids
    FROM rev.playbook_categories pc
    JOIN rev.categories c ON pc.category_id = c.id
    GROUP BY pc.playbook_id
),
args AS (
    SELECT *
    FROM UNNEST($2::UUID[]) WITH ORDINALITY AS o(id, ord)
),
playbook_page AS (
    SELECT p.id
    FROM args
    JOIN rev.playbooks p ON p.id = args.id
    ORDER BY p.id
    LIMIT $1::BIGINT OFFSET $3::BIGINT
)

SELECT
    p.id,
    p.versioned_object_id,
    vo.path_id,
    ppd.name,
    ppd.slug,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    p.description,
    p.created_at,
        pst.id AS "playbook_stage_id?",
    pst.created_at AS "playbook_stage_created_at?",
    s.id AS "playbook_stage_stage_id?",
        ss.id AS "stage_step_id?",
    ss.created_at AS "stage_step_created_at?",
    st.id AS "stage_step_step_id?",
        stk.id AS "step_task_id?",
    t.id AS "step_task_task_id?",
    stk.created_at AS "step_task_created_at?",
    COALESCE(input.category_ids, '{}'::uuid[]) AS category_ids
FROM playbook_page pp
JOIN rev.playbooks p ON p.id = pp.id
JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.playbook_path_details ppd ON ppd.path_id = op.id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.playbook_stages pst ON p.id = pst.playbook_id
LEFT JOIN rev.stages s ON pst.stage_id = s.id
LEFT JOIN rev.stage_steps ss ON s.id = ss.stage_id
LEFT JOIN rev.steps st ON ss.step_id = st.id
LEFT JOIN rev.step_tasks stk ON stk.step_id = st.id
LEFT JOIN rev.tasks t ON t.id = stk.task_id
LEFT JOIN input ON p.id = input.playbook_id
ORDER BY p.id, pst.ord NULLS FIRST, ss.ord NULLS FIRST, stk.ord NULLS FIRST;

