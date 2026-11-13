WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(path_id, ord)
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
JOIN rev.object_paths op ON op.id = args.path_id
JOIN rev.versioned_objects vo ON vo.path_id = op.id
JOIN rev.entity_types et ON et.id = op.entity_type_id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
ORDER BY args.ord, vo.created_at DESC;
