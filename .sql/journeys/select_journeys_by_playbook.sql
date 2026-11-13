-- filepath: crates/dapple-data/assets/sql/journeys/select_journeys_by_playbook.sql
-- Select journeys that belong to the provided playbook (by playbook id)
WITH limited AS (
    SELECT objs.*
    FROM rev.journeys objs
    JOIN rev.playbooks p ON objs.playbook_id = p.id
    WHERE p.id = $1
    ORDER BY objs.id DESC
    LIMIT $2 OFFSET $3
)
SELECT
    limited.id,
    limited.playbook_id,
    ud.object_id AS user_id,
    limited.name,
    limited.created_at,
    -- Brief pack association fields (nullable from LEFT JOIN)
    jbp.id AS "journey_brief_pack_id?",
    bp.id AS "brief_pack_id?"
FROM limited
JOIN rev.playbooks p ON limited.playbook_id = p.id
JOIN rev.user_details ud ON limited.user_id = ud.user_id
LEFT JOIN rev.journey_brief_packs jbp ON jbp.journey_id = limited.id
LEFT JOIN rev.brief_packs bp ON bp.id = jbp.brief_pack_id
ORDER BY limited.id DESC, jbp.id;

