
SELECT
    ps.id AS "playbook_stage_id!",
    spd_stage.name AS "stage_name!",
    ss.id AS "stage_step_id!",
    spd_step.name AS "step_name!"
FROM rev.playbook_stages ps
JOIN rev.stages s ON s.id = ps.stage_id
JOIN rev.versioned_objects vo_s ON vo_s.id = s.versioned_object_id
JOIN rev.stage_path_details spd_stage ON spd_stage.path_id = vo_s.path_id
JOIN rev.stage_steps ss ON ss.stage_id = ps.stage_id
JOIN rev.steps st ON st.id = ss.step_id
JOIN rev.versioned_objects vo_st ON vo_st.id = st.versioned_object_id
JOIN rev.step_path_details spd_step ON spd_step.path_id = vo_st.path_id
WHERE ps.playbook_id = $1
ORDER BY ps.ord, ss.ord;
