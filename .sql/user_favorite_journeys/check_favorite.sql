-- Check if a specific journey is favorited by user
-- $1 = user UUID (rev.user_details.object_id)
-- $2 = journey_id (UUID from rev.journeys.id)
SELECT EXISTS (
    SELECT 1
    FROM web.user_favorite_journeys f
    JOIN rev.user_details ud ON ud.user_id = f.user_id
    LEFT JOIN web.user_favorite_journeys_removed r
        ON r.user_favorite_journeys_id = f.id
    WHERE ud.object_id = $1
      AND f.journey_id = $2
      AND r.user_favorite_journeys_id IS NULL
    ORDER BY f.id DESC
    LIMIT 1
)::bool AS is_favorited;
