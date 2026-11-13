
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[],
        $3::TEXT[]
    ) WITH ORDINALITY
    AS o(
        step_task_id,
        value_slot_path_id,
        visibility,
        ord
    )
),
objs AS (
    INSERT INTO rev.step_task_overlays (step_task_id, value_slot_path_id, visibility)
    SELECT args.step_task_id, args.value_slot_path_id, args.visibility
    FROM args
    ORDER BY args.ord
    RETURNING *
),
numbered_objs AS (
    SELECT o.*, row_number() OVER (ORDER BY o.id) AS ord
    FROM objs o
)
SELECT
    no.id,
    no.step_task_id,
    no.value_slot_path_id,
    no.visibility,
    no.created_at
FROM numbered_objs no
ORDER BY no.id;
