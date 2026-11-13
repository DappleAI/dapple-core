fluent_message_page AS (
    SELECT fm.id
    FROM rev.fluent_messages fm
    JOIN rev.versioned_objects vo ON vo.id = fm.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.fluent_message_path_details fmpd ON fmpd.path_id = op.id
    ORDER BY fm.id
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)
