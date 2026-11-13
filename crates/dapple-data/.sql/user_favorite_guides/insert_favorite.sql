-- Add favorite guide (idempotent: only inserts if no active favorite exists)
-- $1 = user UUID (rev.user_details.object_id)
-- $2 = guide_path_id (UUID from rev.guide_path_details.path_id)
WITH user_lookup AS (
    SELECT ud.user_id, ud.object_id
    FROM rev.user_details ud
    WHERE ud.object_id = $1
),
path_lookup AS (
    SELECT op.id AS path_id, gpd.title
    FROM rev.object_paths op
    JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
    WHERE op.id = $2
),
active_check AS (
    SELECT f.id
    FROM web.user_favorite_guides f
    LEFT JOIN web.user_favorite_guides_removed r
        ON r.user_favorite_guides_id = f.id
    WHERE f.user_id = (SELECT user_id FROM user_lookup)
      AND f.guide_path_id = (SELECT path_id FROM path_lookup)
      AND r.user_favorite_guides_id IS NULL
    ORDER BY f.id DESC
    LIMIT 1
),
inserted AS (
    INSERT INTO web.user_favorite_guides (user_id, guide_path_id)
    SELECT (SELECT user_id FROM user_lookup), (SELECT path_id FROM path_lookup)
    WHERE NOT EXISTS (SELECT 1 FROM active_check)
      AND EXISTS (SELECT 1 FROM path_lookup)
      AND EXISTS (SELECT 1 FROM user_lookup)
    RETURNING id, user_id, guide_path_id, created_at
)
SELECT
    i.id,
    ul.object_id AS user_id,
    i.guide_path_id,
    pl.title AS guide_title,
    i.created_at
FROM inserted i
JOIN path_lookup pl ON pl.path_id = i.guide_path_id
JOIN user_lookup ul ON ul.user_id = i.user_id;
