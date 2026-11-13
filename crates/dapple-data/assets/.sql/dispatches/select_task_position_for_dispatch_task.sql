WITH target AS (
    SELECT j.playbook_id
    FROM rev.dispatches d
    JOIN rev.journeys j ON j.id = d.journey_id
    WHERE d.id = $1::UUID
)
SELECT
    spd.name AS "stage_name!",
    COALESCE(rev.fluent_body(s.description_fluent_message_id, 'en'), s.description, spd.name) AS "stage_description!",
    stpd.name AS "step_name!",
    COALESCE(rev.fluent_body(st.description_fluent_message_id, 'en'), st.description, stpd.name) AS "step_description!",
    ps.ord AS stage_ord,
    (SELECT COUNT(*) FROM rev.playbook_stages p2 WHERE p2.playbook_id = t.playbook_id) AS "total_stages!"
FROM rev.step_tasks stk
JOIN rev.stage_steps ss ON ss.step_id = stk.step_id
JOIN rev.playbook_stages ps ON ps.stage_id = ss.stage_id
JOIN target t ON t.playbook_id = ps.playbook_id
JOIN rev.stages s ON s.id = ps.stage_id
JOIN rev.versioned_objects vo_s ON vo_s.id = s.versioned_object_id
JOIN rev.stage_path_details spd ON spd.path_id = vo_s.path_id
JOIN rev.steps st ON st.id = ss.step_id
JOIN rev.versioned_objects vo_st ON vo_st.id = st.versioned_object_id
JOIN rev.step_path_details stpd ON stpd.path_id = vo_st.path_id
WHERE stk.id = $2::UUID
ORDER BY ps.ord NULLS LAST, ss.ord NULLS LAST
LIMIT 1;
