stage_page AS (
    SELECT st.id
    FROM rev.stages st
    JOIN rev.versioned_objects vo ON vo.id = st.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.stage_path_details spd ON spd.path_id = op.id
    ORDER BY st.id
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)
