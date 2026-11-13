-- Select versioned objects by IDs with full joins
WITH latest_deprecation AS (
    SELECT DISTINCT ON (versioned_object_id)
        versioned_object_id,
        to_level_id
    FROM rev.deprecation_events
    ORDER BY versioned_object_id, created_at DESC
),
args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    vo.id,
    vo.path_id,
    op.entity_type_id,
    et.key AS entity_type_key,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    vo.created_at
FROM args
JOIN rev.versioned_objects vo ON vo.id = args.id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.entity_types et ON et.id = op.entity_type_id
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
ORDER BY vo.id;
