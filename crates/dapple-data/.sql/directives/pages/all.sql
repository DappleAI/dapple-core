directive_page AS (
    SELECT d.id
    FROM rev.directives d
    JOIN rev.versioned_objects vo ON vo.id = d.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.directive_path_details dpd ON dpd.path_id = op.id
    ORDER BY d.id
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)
