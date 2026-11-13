brief_page AS (
    SELECT b.id
    FROM rev.briefs b
    JOIN rev.versioned_objects vo ON vo.id = b.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.brief_path_details bpd ON bpd.path_id = op.id
    ORDER BY b.id
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)
