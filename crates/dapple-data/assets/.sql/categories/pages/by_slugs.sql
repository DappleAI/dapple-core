args AS (
    SELECT *
    FROM UNNEST($3::TEXT[]) WITH ORDINALITY AS o(slug, ord)
),
category_page AS (
    SELECT c.id, args.ord
    FROM args
    JOIN rev.category_path_details cpd ON cpd.slug = args.slug
    JOIN rev.object_paths op ON op.id = cpd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.categories c ON c.versioned_object_id = vo.id
    ORDER BY args.ord
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)
