SELECT d.id
FROM rev.dispatch_steps s
JOIN rev.dispatch_stages st ON s.dispatch_stage_id = st.id
JOIN rev.dispatches d ON st.dispatch_id = d.id
WHERE s.id = $1::uuid;

