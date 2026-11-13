-- External-API fluent_message page CTE: list fluent messages scoped to the
-- caller's company namespace. Non-deprecated only.
fluent_message_latest AS (
    SELECT DISTINCT ON (op.id)
        fm.id
    FROM rev.fluent_messages fm
    JOIN rev.versioned_objects vo ON vo.id = fm.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
    LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
    WHERE dl.id IS NULL
      AND op.namespace_company_id = $1::UUID
    ORDER BY op.id, vo.created_at DESC
),
fluent_message_page AS (
    SELECT id FROM fluent_message_latest
    ORDER BY id
    LIMIT $2::BIGINT OFFSET $3::BIGINT
)
