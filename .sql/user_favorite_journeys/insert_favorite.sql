-- Add favorite (idempotent: only inserts if no active favorite exists)
-- $1 = user UUID (rev.user_details.object_id)
-- $2 = journey_id (UUID from rev.journeys.id)
WITH user_lookup AS (
    SELECT ud.user_id, ud.object_id
    FROM rev.user_details ud
    WHERE ud.object_id = $1
),
journey_lookup AS (
    SELECT id, name FROM rev.journeys WHERE id = $2
),
active_check AS (
    SELECT f.id
    FROM web.user_favorite_journeys f
    LEFT JOIN web.user_favorite_journeys_removed r
        ON r.user_favorite_journeys_id = f.id
    WHERE f.user_id = (SELECT user_id FROM user_lookup)
      AND f.journey_id = $2
      AND r.user_favorite_journeys_id IS NULL
    ORDER BY f.id DESC
    LIMIT 1
),
inserted AS (
    INSERT INTO web.user_favorite_journeys (user_id, journey_id)
    SELECT (SELECT user_id FROM user_lookup), $2
    WHERE NOT EXISTS (SELECT 1 FROM active_check)
      AND EXISTS (SELECT 1 FROM journey_lookup)
      AND EXISTS (SELECT 1 FROM user_lookup)
    RETURNING id, user_id, journey_id, created_at
)
SELECT
    i.id,
    ul.object_id AS user_id,
    i.journey_id,
    jl.name AS journey_name,
    i.created_at
FROM inserted i
JOIN journey_lookup jl ON jl.id = i.journey_id
JOIN user_lookup ul ON ul.user_id = i.user_id;
