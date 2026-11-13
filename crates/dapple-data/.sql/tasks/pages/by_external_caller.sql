-- External-API task page CTE: list tasks scoped to the caller's
-- company namespace. Non-deprecated only.
task_latest AS (
    SELECT DISTINCT ON (op.id)
        t.id
    FROM rev.tasks t
    JOIN rev.versioned_objects vo ON vo.id = t.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
    LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
    WHERE dl.id IS NULL
      AND op.namespace_company_id = $1::UUID
    ORDER BY op.id, vo.created_at DESC
),
task_page AS (
    SELECT id FROM task_latest
    ORDER BY id
    LIMIT $2::BIGINT OFFSET $3::BIGINT
)
