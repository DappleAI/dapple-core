-- Get all active favorites for a user (with journey id for DTO)
-- $1 = user UUID (rev.user_details.object_id)
WITH user_lookup AS (
    SELECT ud.user_id, ud.object_id
    FROM rev.user_details ud
    WHERE ud.object_id = $1
),
latest AS (
    SELECT DISTINCT ON (f.user_id, f.journey_id)
        f.id, f.user_id, f.journey_id, f.created_at
    FROM web.user_favorite_journeys f
    WHERE f.user_id = (SELECT user_id FROM user_lookup)
    ORDER BY f.user_id, f.journey_id, f.id DESC
)
SELECT
    l.id,
    ul.object_id AS user_id,
    l.journey_id,
    j.name AS journey_name,
    l.created_at
FROM latest l
JOIN rev.journeys j ON j.id = l.journey_id
JOIN user_lookup ul ON ul.user_id = l.user_id
LEFT JOIN web.user_favorite_journeys_removed r
    ON r.user_favorite_journeys_id = l.id
WHERE r.user_favorite_journeys_id IS NULL
ORDER BY l.created_at DESC
LIMIT $2 OFFSET $3;
