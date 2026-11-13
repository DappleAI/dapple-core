-- Insert playbook_stage_overlays
-- Parameters:
--   $1: playbook_stage_ids (UUID[])
--   $2: value_slot_path_ids (UUID[])
--   $3: visibilities (TEXT[]) - 'expose' or 'mask'

WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[],
        $3::TEXT[]
    ) WITH ORDINALITY
    AS o(
        playbook_stage_id,
        value_slot_path_id,
        visibility,
        ord
    )
),
objs AS (
    INSERT INTO rev.playbook_stage_overlays (playbook_stage_id, value_slot_path_id, visibility)
    SELECT args.playbook_stage_id, args.value_slot_path_id, args.visibility
    FROM args
    ORDER BY args.ord
    RETURNING *
),
-- Number objs to preserve ordinality
numbered_objs AS (
    SELECT o.*, row_number() OVER (ORDER BY o.id) AS ord
    FROM objs o
)
SELECT
    no.id,
    no.playbook_stage_id,
    no.value_slot_path_id,
    no.visibility,
    no.created_at
FROM numbered_objs no
ORDER BY no.id;
