SELECT EXISTS (
    SELECT 1
    FROM web.user_favorite_guides f
    JOIN rev.user_details ud ON ud.user_id = f.user_id
    JOIN rev.guide_path_details gpd ON gpd.path_id = f.guide_path_id
    LEFT JOIN web.user_favorite_guides_removed r
        ON r.user_favorite_guides_id = f.id
    WHERE ud.object_id = $1
      AND f.guide_path_id = $2
      AND r.user_favorite_guides_id IS NULL
    ORDER BY f.id DESC
    LIMIT 1
)::bool AS is_favorited;
