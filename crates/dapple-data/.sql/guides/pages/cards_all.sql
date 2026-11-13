current_pins AS (
    SELECT crp.ref_key, crp.versioned_object_id, crp.id AS pin_id
    FROM rev.canonical_release_pins crp
    LEFT JOIN rev.canonical_release_pins newer ON newer.supersedes_id = crp.id
    WHERE newer.id IS NULL
      AND crp.scope = $2::TEXT
      AND crp.environment = $1::TEXT
      AND crp.object_type = 'guides'
      AND crp.resolved_at IS NOT NULL
),
guide_card_page AS (
    SELECT DISTINCT ON (op.id)
        g.id AS guide_id,
        vo.path_id,
        gpd.title,
        gpd.slug,
        vo.content_hash
    FROM rev.guides g
    JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
    LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
    LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
    LEFT JOIN current_pins cp
        ON cp.ref_key = g.source_path
       AND cp.versioned_object_id = vo.id
    WHERE dl.id IS NULL
    ORDER BY op.id,
        cp.pin_id NULLS LAST,
        vo.created_at DESC
)
