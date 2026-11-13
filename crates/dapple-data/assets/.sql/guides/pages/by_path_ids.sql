args AS (
    SELECT *
    FROM UNNEST($3::UUID[]) WITH ORDINALITY AS o(path_id, ord)
),
guide_page AS (
    SELECT g.id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.guides g ON g.versioned_object_id = vo.id
    ORDER BY g.id DESC
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)
