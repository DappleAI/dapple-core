-- Get all active favorite guides for a user (with path id and title for DTO)
-- $1 = user UUID (rev.user_details.object_id)
WITH user_lookup AS (
    SELECT ud.user_id, ud.object_id
    FROM rev.user_details ud
    WHERE ud.object_id = $1
),
latest AS (
    SELECT DISTINCT ON (f.user_id, f.guide_path_id)
        f.id, f.user_id, f.guide_path_id, f.created_at
    FROM web.user_favorite_guides f
    WHERE f.user_id = (SELECT user_id FROM user_lookup)
    ORDER BY f.user_id, f.guide_path_id, f.id DESC
)
SELECT
    l.id,
    ul.object_id AS user_id,
    l.guide_path_id,
    gpd.title AS guide_title,
    l.created_at
FROM latest l
JOIN rev.guide_path_details gpd ON gpd.path_id = l.guide_path_id
JOIN user_lookup ul ON ul.user_id = l.user_id
LEFT JOIN web.user_favorite_guides_removed r
    ON r.user_favorite_guides_id = l.id
WHERE r.user_favorite_guides_id IS NULL
ORDER BY l.created_at DESC
LIMIT $2 OFFSET $3;
