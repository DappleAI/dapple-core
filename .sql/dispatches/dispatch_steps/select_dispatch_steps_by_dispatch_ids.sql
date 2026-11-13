SELECT
    ds.id AS id,
    d.id AS dispatch_id,
    dst.id AS dispatch_stage_id,
    ds.stage_step_id,
    pst.id AS playbook_stage_id,
    ss.step_id AS step_id,
    pst.playbook_id AS playbook_id,
    ds.params,
    ds.created_at,
    dt.id AS "dispatch_task_id?",
    dt.step_task_id AS "dispatch_task_step_task_id?",
    t.id AS "dispatch_task_task_id?",
    t.created_at AS "dispatch_task_created_at?"
FROM rev.dispatch_steps ds
JOIN rev.dispatch_stages dst ON ds.dispatch_stage_id = dst.id
JOIN rev.dispatches d ON dst.dispatch_id = d.id
JOIN rev.stage_steps ss ON ds.stage_step_id = ss.id
JOIN rev.playbook_stages pst ON ss.stage_id = pst.stage_id
    AND dst.playbook_stage_id = pst.id
LEFT JOIN rev.dispatch_tasks dt ON dt.dispatch_step_id = ds.id
LEFT JOIN rev.step_tasks stk ON dt.step_task_id = stk.id
LEFT JOIN rev.tasks t ON stk.task_id = t.id
WHERE d.id = $1;
