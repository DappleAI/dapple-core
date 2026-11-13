SELECT
    ds.id AS id,
    d.id AS dispatch_id,
    d_st.params AS params,
    ds.stage_step_id AS "dispatch_step_stage_step_id?",
    s.id AS "step_id?",
    dst_pst.playbook_id AS "playbook_id?",
    ds.params AS "dispatch_step_params?",
    ds.created_at AS "dispatch_step_created_at?",
    dt.id AS "dispatch_task_id?",
    dt.step_task_id AS "dispatch_task_step_task_id?",
    t.id AS "dispatch_task_task_id?",
    t.created_at AS "dispatch_task_created_at?"
FROM rev.dispatch_stages d_st
LEFT JOIN rev.dispatch_steps ds ON d_st.id = ds.dispatch_stage_id
JOIN rev.dispatches d ON d_st.dispatch_id = d.id
JOIN rev.playbook_stages dst_pst ON d_st.playbook_stage_id = dst_pst.id
LEFT JOIN rev.stage_steps ss ON ds.stage_step_id = ss.id
LEFT JOIN rev.steps s ON ss.step_id = s.id
LEFT JOIN rev.dispatch_tasks dt ON dt.dispatch_step_id = ds.id
LEFT JOIN rev.step_tasks stk ON dt.step_task_id = stk.id
LEFT JOIN rev.tasks t ON stk.task_id = t.id
WHERE d.id = $1;
