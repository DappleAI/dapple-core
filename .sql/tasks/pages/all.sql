task_page AS (
    SELECT t.id
    FROM rev.tasks t
    JOIN rev.versioned_objects vo ON vo.id = t.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.task_path_details tpd ON tpd.path_id = op.id
    ORDER BY t.id
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)
