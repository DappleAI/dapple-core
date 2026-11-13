-- External-API page CTE: list guides scoped to the caller's company namespace.
--
-- Customers do not interact with rev.canonical_release_pins (pinning is a
-- dapple-team release-management concern, not a customer one), so this query
-- does not consult the pin table. Each path's "current" version is simply the
-- latest non-deprecated `versioned_objects` row.
--
-- The customer's surface filters by `(scope = 'external', namespace_company_id
-- = caller's UUID)` at the object_paths layer. There is no pin layer for
-- external content; if and when customer-side pinning ever becomes a real
-- requirement, design it then with a customer-friendly model.
guide_latest AS (
    SELECT DISTINCT ON (op.id)
        g.id
    FROM rev.guides g
    JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
    LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
    LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
    WHERE dl.id IS NULL
      AND op.namespace_company_id = $1::UUID
      AND op.scope = 'external'
    ORDER BY op.id, vo.created_at DESC
),
guide_page AS (
    SELECT id FROM guide_latest
    ORDER BY id
    LIMIT $2::BIGINT OFFSET $3::BIGINT
)
