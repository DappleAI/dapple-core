-- Select all directives using unified versioning
-- Shared body template for directive SELECT queries.
-- Callers provide a @page slot defining one or more CTEs
-- that produce a `directive_page` CTE with at minimum `directive_page.id`.
WITH latest_deprecation AS (
    SELECT DISTINCT ON (versioned_object_id)
        versioned_object_id,
        to_level_id
    FROM rev.deprecation_events
    ORDER BY versioned_object_id, created_at DESC
),

directive_page AS (
    SELECT d.id
    FROM rev.directives d
    JOIN rev.versioned_objects vo ON vo.id = d.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.directive_path_details dpd ON dpd.path_id = op.id
    ORDER BY d.id
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)

SELECT
    d.id,
    d.versioned_object_id,
    op.id AS path_id,
    dpd.name,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    d.logic,
    d.max_attempts,
    d.retry_delay_ms,
    d.description,
    d.created_at
FROM directive_page dkp
JOIN rev.directives d ON d.id = dkp.id
JOIN rev.versioned_objects vo ON vo.id = d.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.directive_path_details dpd ON dpd.path_id = op.id
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
ORDER BY d.id;

