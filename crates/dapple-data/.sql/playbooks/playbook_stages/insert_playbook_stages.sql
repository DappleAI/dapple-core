WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],  -- playbook_id[]
        $2::UUID[]   -- stage_id[]
    ) WITH ORDINALITY AS o(playbook_id, stage_id, ord)
),
-- Resolve IDs with ordinality preserved
resolved AS (
    SELECT
        args.ord,
        args.playbook_id,
        s.id AS stage_id
    FROM args
    JOIN rev.playbooks p ON p.id = args.playbook_id
    JOIN rev.stages s ON s.id = args.stage_id
),
objs AS (
    INSERT INTO rev.playbook_stages (playbook_id, stage_id)
    SELECT r.playbook_id, r.stage_id
    FROM resolved r
    ORDER BY r.ord
    RETURNING *
),
-- Number objs to preserve ordinality
numbered_objs AS (
    SELECT o.*, row_number() OVER (ORDER BY o.id) AS ord
    FROM objs o
)
SELECT
    no.id,
    no.playbook_id,
    no.stage_id,
    no.created_at
FROM numbered_objs no
JOIN resolved r ON r.ord = no.ord
ORDER BY no.id;
