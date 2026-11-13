WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],  -- playbook_id[]
        $2::UUID[],  -- user_object_id[]
        $3::TEXT[]     -- name[]
    ) WITH ORDINALITY AS o(playbook_id, user_object_id, name, ord)
),
-- Resolve playbook and user IDs with ordinality preserved
resolved_args AS (
    SELECT
        args.ord,
        args.name,
        p.id AS playbook_id,
        ud.user_id,
        ud.object_id AS user_object_id
    FROM args
    JOIN rev.playbooks p ON args.playbook_id = p.id
    JOIN LATERAL (
        SELECT * FROM rev.user_details ud_sub
        WHERE ud_sub.object_id = args.user_object_id
        LIMIT 1
    ) ud ON true
),
objs AS (
    INSERT INTO rev.journeys (playbook_id, user_id, name)
    SELECT ra.playbook_id, ra.user_id, ra.name
    FROM resolved_args ra
    ORDER BY ra.ord
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
    ra.user_object_id AS user_id,
    no.name,
    no.created_at
FROM numbered_objs no
JOIN resolved_args ra ON ra.ord = no.ord
ORDER BY no.id;

