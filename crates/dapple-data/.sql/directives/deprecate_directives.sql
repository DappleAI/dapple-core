-- Deprecate directives by their path IDs
-- Creates deprecation events for the latest version of each directive path
WITH args AS (
    SELECT *
    FROM UNNEST(
        $2::UUID[],
        $3::TEXT[]
    ) WITH ORDINALITY AS o(path_id, reason, ord)
),
latest_deprecation AS (
    SELECT DISTINCT ON (versioned_object_id)
        versioned_object_id,
        to_level_id
    FROM rev.deprecation_events
    ORDER BY versioned_object_id, created_at DESC
),
latest_versions AS (
    SELECT DISTINCT ON (op.id)
        op.id AS path_id,
        vo.id AS versioned_object_id,
        ld.to_level_id AS from_level_id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
    ORDER BY op.id, vo.created_at DESC
),
resolved AS (
    SELECT lv.versioned_object_id, lv.from_level_id, lv.path_id, args.reason
    FROM args
    JOIN latest_versions lv ON lv.path_id = args.path_id
),

events AS (
    INSERT INTO rev.deprecation_events (
        versioned_object_id, from_level_id, to_level_id, reason, deprecated_by_user_id
    )
    SELECT
        resolved.versioned_object_id,
        resolved.from_level_id,
        $4::SMALLINT,
        resolved.reason,
        $1::UUID
    FROM resolved
    RETURNING *
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
FROM resolved
JOIN rev.versioned_objects vo ON vo.id = resolved.versioned_object_id
JOIN rev.object_paths op ON op.id = resolved.path_id
JOIN rev.directive_path_details dpd ON dpd.path_id = op.id
JOIN rev.directives d ON d.versioned_object_id = resolved.versioned_object_id
LEFT JOIN rev.deprecation_levels dl ON dl.id = $4::SMALLINT
ORDER BY vo.id;
