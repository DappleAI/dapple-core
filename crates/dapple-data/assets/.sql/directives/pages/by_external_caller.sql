directive_latest AS (
    SELECT DISTINCT ON (op.id)
        d.id
    FROM rev.directives d
    JOIN rev.versioned_objects vo ON vo.id = d.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    LEFT JOIN LATERAL (
        SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

    ) ld ON TRUE
    LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
    WHERE dl.id IS NULL
      AND op.namespace_company_id = $1::UUID
    ORDER BY op.id, vo.created_at DESC
),
directive_page AS (
    SELECT id FROM directive_latest
    ORDER BY id
    LIMIT $2::BIGINT OFFSET $3::BIGINT
)
