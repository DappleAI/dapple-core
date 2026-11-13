args AS (
    SELECT *
    FROM UNNEST($3::TEXT[]) WITH ORDINALITY AS o(slug, ord)
),
step_page AS (
    SELECT s.id, args.ord
    FROM args
    JOIN rev.step_path_details spd ON spd.slug = args.slug
    JOIN rev.object_paths op ON op.id = spd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.steps s ON s.versioned_object_id = vo.id
    ORDER BY args.ord
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)
