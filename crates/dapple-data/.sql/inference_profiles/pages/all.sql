inference_profile_page AS (
    SELECT ip.id
    FROM rev.inference_profiles ip
    JOIN rev.versioned_objects vo ON vo.id = ip.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.inference_profile_path_details ipd ON ipd.path_id = op.id
    ORDER BY ip.id
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)
