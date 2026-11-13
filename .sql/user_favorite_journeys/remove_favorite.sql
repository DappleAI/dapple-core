-- Remove favorite (idempotent: only removes if active favorite exists)
-- $1 = user UUID (rev.user_details.object_id)
-- $2 = journey_id (UUID from rev.journeys.id)
WITH user_lookup AS (
    SELECT ud.user_id
    FROM rev.user_details ud
    WHERE ud.object_id = $1
),
active_favorite AS (
    SELECT f.id
    FROM web.user_favorite_journeys f
    LEFT JOIN web.user_favorite_journeys_removed r
        ON r.user_favorite_journeys_id = f.id
    WHERE f.user_id = (SELECT user_id FROM user_lookup)
      AND f.journey_id = $2
      AND r.user_favorite_journeys_id IS NULL
    ORDER BY f.id DESC
    LIMIT 1
)
INSERT INTO web.user_favorite_journeys_removed (user_favorite_journeys_id, removed_at)
SELECT id, now()
FROM active_favorite
RETURNING user_favorite_journeys_id, removed_at;
