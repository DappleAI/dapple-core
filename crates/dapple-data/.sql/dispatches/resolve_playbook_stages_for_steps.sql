-- Resolve stage_step_ids to their parent playbook_stage_ids.
-- Returns one row per input step, with the step and stage identifiers.
WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(stage_step_id, ord)
)
SELECT
    args.ord,
    ss.id AS stage_step_id,
    pst.id AS playbook_stage_id
FROM args
JOIN rev.stage_steps ss ON ss.id = args.stage_step_id
JOIN rev.playbook_stages pst ON ss.stage_id = pst.stage_id
ORDER BY args.ord;
