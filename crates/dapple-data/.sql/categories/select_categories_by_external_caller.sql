-- External-API category listing: scoped to the caller's company namespace.
-- Used by GET /api/categories on external-api.dapple.x.
-- Shared body template for category SELECT queries.
-- Callers provide a @page slot defining one or more CTEs
-- that produce a `category_page` CTE with at minimum `category_page.id`.
WITH latest_deprecation AS (
    SELECT DISTINCT ON (versioned_object_id)
        versioned_object_id,
        to_level_id
    FROM rev.deprecation_events
    ORDER BY versioned_object_id, created_at DESC
),

-- External-API category page CTE: list categories scoped to the caller's
-- company namespace. Non-deprecated only.
category_latest AS (
    SELECT DISTINCT ON (op.id)
        c.id
    FROM rev.categories c
    JOIN rev.versioned_objects vo ON vo.id = c.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
    LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
    WHERE dl.id IS NULL
      AND op.namespace_company_id = $1::UUID
    ORDER BY op.id, vo.created_at DESC
),
category_page AS (
    SELECT id FROM category_latest
    ORDER BY id
    LIMIT $2::BIGINT OFFSET $3::BIGINT
)

SELECT
    c.id,
    c.versioned_object_id,
    vo.path_id,
    cpd.name,
    cpd.slug,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    c.description,
    c.created_at
FROM category_page ckp
JOIN rev.categories c ON c.id = ckp.id
JOIN rev.versioned_objects vo ON vo.id = c.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.category_path_details cpd ON cpd.path_id = op.id
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
ORDER BY c.id;

