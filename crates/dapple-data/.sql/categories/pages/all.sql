category_page AS (
    SELECT c.id
    FROM rev.categories c
    JOIN rev.versioned_objects vo ON vo.id = c.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.category_path_details cpd ON cpd.path_id = op.id
    ORDER BY c.id
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)
