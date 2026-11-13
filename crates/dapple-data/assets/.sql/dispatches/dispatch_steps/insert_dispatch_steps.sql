WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],             $2::UUID[],             $3::JSONB[]         ) WITH ORDINALITY AS o(dispatch_stage_id, stage_step_id, params, ord)
),
input AS (
    SELECT
        a.*,
        ss.id AS resolved_stage_step_id
    FROM args a
    LEFT JOIN rev.stage_steps ss ON ss.id = a.stage_step_id
),
objs AS (
    INSERT INTO rev.dispatch_steps (dispatch_stage_id, stage_step_id, params)
    SELECT input.dispatch_stage_id, input.resolved_stage_step_id, input.params
    FROM input
    ORDER BY input.ord
    RETURNING *
)
SELECT
    objs.*,
    pst.id AS playbook_stage_id,
    d.id AS dispatch_id,
    ss.step_id AS step_id,
    pst.playbook_id AS playbook_id
FROM objs
JOIN rev.stage_steps ss ON objs.stage_step_id = ss.id
JOIN rev.dispatch_stages dst ON objs.dispatch_stage_id = dst.id
JOIN rev.dispatches d ON dst.dispatch_id = d.id
JOIN rev.playbook_stages pst ON ss.stage_id = pst.stage_id
    AND dst.playbook_stage_id = pst.id
JOIN input ON objs.dispatch_stage_id = input.dispatch_stage_id
    AND objs.stage_step_id = input.resolved_stage_step_id
    AND objs.params = input.params
ORDER BY objs.dispatch_stage_id, objs.id;
