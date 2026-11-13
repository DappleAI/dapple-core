WITH user_lookup AS (
    SELECT ud.user_id, ud.object_id
    FROM rev.user_details ud
    WHERE ud.object_id = $3::UUID
),
latest AS (
    SELECT DISTINCT ON (f.user_id, f.chain_root_journey_id)
        f.id, f.user_id, f.chain_root_journey_id, f.created_at
    FROM web.user_favorite_journey_chains f
    WHERE f.user_id = (SELECT user_id FROM user_lookup)
    ORDER BY f.user_id, f.chain_root_journey_id, f.id DESC
),
viewer_prefs AS (
    SELECT DISTINCT ON (pjp.journey_id)
        pjp.journey_id,
        pjp.name AS name_override
    FROM rev.principal_journey_preferences pjp
    WHERE pjp.principal_id = $4::UUID
    ORDER BY pjp.journey_id, pjp.id DESC
)
SELECT
    l.id,
    ul.object_id AS user_id,
    l.chain_root_journey_id,
    COALESCE(vp.name_override, j.name) AS "chain_root_name!",
    l.created_at
FROM latest l
JOIN rev.journeys j ON j.id = l.chain_root_journey_id
JOIN user_lookup ul ON ul.user_id = l.user_id
LEFT JOIN viewer_prefs vp ON vp.journey_id = l.chain_root_journey_id
LEFT JOIN web.user_favorite_journey_chains_removed r
    ON r.user_favorite_journey_chains_id = l.id
WHERE r.user_favorite_journey_chains_id IS NULL
ORDER BY l.created_at DESC
LIMIT $1::BIGINT OFFSET $2::BIGINT;
