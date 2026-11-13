args AS (
    SELECT *
    FROM UNNEST($3::TEXT[]) WITH ORDINALITY AS o(slug, ord)
),
hat_page AS (
    SELECT h.id
    FROM args
    JOIN rev.hat_path_details hpd ON hpd.slug = args.slug
    JOIN rev.object_paths op ON op.id = hpd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.hats h ON h.versioned_object_id = vo.id
    ORDER BY args.ord
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)
