brief_pack_page AS (
    SELECT bp.id
    FROM rev.brief_packs bp
    JOIN rev.versioned_objects vo ON vo.id = bp.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.brief_pack_path_details bppd ON bppd.path_id = op.id
    ORDER BY bp.id
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)
