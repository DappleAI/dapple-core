WITH limited AS (
    SELECT d.*
    FROM rev.dispatches d
    WHERE d.journey_id = (SELECT id FROM rev.journeys WHERE id = $1)
    ORDER BY d.id DESC
    LIMIT $2 OFFSET $3
)
SELECT
    d.id,
    d.journey_id,
    d.intent,
    d.created_at,
    dst.id AS "dispatch_stage_id?",
    dst.playbook_stage_id AS "dispatch_stage_playbook_stage_id?",
    dst_pst.playbook_id AS "dispatch_stage_playbook_id?",
    dst_pst.stage_id AS "dispatch_stage_stage_id?",
    dst.params AS "dispatch_stage_params?",
    dst.created_at AS "dispatch_stage_created_at?",
    ds.id AS "dispatch_step_id?",
    ds.stage_step_id AS "dispatch_step_stage_step_id?",
    dst.playbook_stage_id AS "dispatch_step_playbook_stage_id?",
    s.id AS "dispatch_step_step_id?",
    dst_pst.playbook_id AS "dispatch_step_playbook_id?",
    ds.params AS "dispatch_step_params?",
    ds.created_at AS "dispatch_step_created_at?",
    dt.id AS "dispatch_task_id?",
    dt.step_task_id AS "dispatch_task_step_task_id?",
    t.id AS "dispatch_task_task_id?",
    dt.created_at AS "dispatch_task_created_at?",
    dta.status AS "dispatch_task_status?"
FROM limited d
JOIN rev.journeys j ON d.journey_id = j.id
LEFT JOIN rev.dispatch_stages dst ON d.id = dst.dispatch_id
LEFT JOIN rev.playbook_stages dst_pst ON dst.playbook_stage_id = dst_pst.id
LEFT JOIN rev.playbooks dp ON dst_pst.playbook_id = dp.id
LEFT JOIN rev.stages ds2 ON dst_pst.stage_id = ds2.id
LEFT JOIN rev.dispatch_steps ds ON dst.id = ds.dispatch_stage_id
LEFT JOIN rev.stage_steps ss ON ds.stage_step_id = ss.id
LEFT JOIN rev.steps s ON ss.step_id = s.id
LEFT JOIN rev.dispatch_tasks dt ON dt.dispatch_step_id = ds.id
LEFT JOIN rev.step_tasks stk ON dt.step_task_id = stk.id
LEFT JOIN rev.tasks t ON stk.task_id = t.id
LEFT JOIN LATERAL (
    SELECT COALESCE(ev.phase_code, a.status) as status
    FROM rev.dispatch_task_attempts a
    LEFT JOIN LATERAL (
        SELECT phase_code
        FROM rev.dispatch_task_attempt_events ev2
        WHERE ev2.attempt_id = a.id
        ORDER BY ev2.created_at DESC, ev2.id DESC
        LIMIT 1
    ) ev ON true
    WHERE a.dispatch_step_id = ds.id
      AND a.task_id = t.id
    ORDER BY a.created_at DESC
    LIMIT 1
) dta ON true
-- Outer ORDER BY must be ASC on d.id so FlatDispatchModel::fold_models /
-- MergeOrdered can walk parents and children in matching ascending order.
-- Newest-first selection is preserved by the inner `limited` CTE's DESC LIMIT,
-- and the Rust caller reverses the final Vec to expose newest-first to API consumers.
ORDER BY d.id, dst.id NULLS FIRST, ds.id NULLS FIRST, dt.id NULLS FIRST;
