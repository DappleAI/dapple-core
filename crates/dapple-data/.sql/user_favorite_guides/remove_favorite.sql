-- Remove favorite guide (idempotent: only removes if active favorite exists)
-- $1 = user UUID (rev.user_details.object_id)
-- $2 = guide_path_id (UUID from rev.guide_path_details.path_id)
WITH user_lookup AS (
    SELECT ud.user_id
    FROM rev.user_details ud
    WHERE ud.object_id = $1
),
path_lookup AS (
    SELECT op.id AS path_id
    FROM rev.object_paths op
    JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
    WHERE op.id = $2
),
active_favorite AS (
    SELECT f.id
    FROM web.user_favorite_guides f
    LEFT JOIN web.user_favorite_guides_removed r
        ON r.user_favorite_guides_id = f.id
    WHERE f.user_id = (SELECT user_id FROM user_lookup)
      AND f.guide_path_id = (SELECT path_id FROM path_lookup)
      AND r.user_favorite_guides_id IS NULL
    ORDER BY f.id DESC
    LIMIT 1
)
INSERT INTO web.user_favorite_guides_removed (user_favorite_guides_id, removed_at)
SELECT id, now()
FROM active_favorite
RETURNING user_favorite_guides_id, removed_at;
