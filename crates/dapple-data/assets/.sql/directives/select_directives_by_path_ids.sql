WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(path_id, ord)
),
directive_page AS (
    SELECT d.id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.directive_path_details dpd ON dpd.path_id = op.id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.directives d ON d.versioned_object_id = vo.id
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
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
ORDER BY d.id;

