-- External-API stage listing: scoped to the caller's company namespace.
-- Used by GET /api/stages on external-api.dapple.x.
-- Shared body template for stage SELECT queries.
-- Callers provide a @page slot defining one or more CTEs
-- that produce a `stage_page` CTE with at minimum `stage_page.id`.
WITH latest_deprecation AS (
    SELECT DISTINCT ON (versioned_object_id)
        versioned_object_id,
        to_level_id
    FROM rev.deprecation_events
    ORDER BY versioned_object_id, created_at DESC
),

-- External-API stage page CTE: list stages scoped to the caller's
-- company namespace. Non-deprecated only.
stage_latest AS (
    SELECT DISTINCT ON (op.id)
        st.id
    FROM rev.stages st
    JOIN rev.versioned_objects vo ON vo.id = st.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
    LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
    WHERE dl.id IS NULL
      AND op.namespace_company_id = $1::UUID
    ORDER BY op.id, vo.created_at DESC
),
stage_page AS (
    SELECT id FROM stage_latest
    ORDER BY id
    LIMIT $2::BIGINT OFFSET $3::BIGINT
)

SELECT
    st.id,
    st.versioned_object_id,
    vo.path_id,
    spd.name,
    spd.slug,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    st.description,
    st.created_at
FROM stage_page stp
JOIN rev.stages st ON st.id = stp.id
JOIN rev.versioned_objects vo ON vo.id = st.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.stage_path_details spd ON spd.path_id = op.id
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
ORDER BY st.id;

