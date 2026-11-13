WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(path_id, ord)
),
ranked AS (
    SELECT
        vo.id,
        vo.path_id,
        vo.content_hash,
        vo.created_at,
        ROW_NUMBER() OVER (
            PARTITION BY vo.path_id
            ORDER BY vo.created_at DESC
        ) AS rn
    FROM args
    JOIN rev.versioned_objects vo ON vo.path_id = args.path_id
)
SELECT
    ranked.id,
    ranked.path_id,
    op.entity_type_id,
    et.key AS entity_type_key,
    ranked.content_hash,
    dl.key AS "deprecation_level_key?",
    ranked.created_at
FROM ranked
JOIN rev.object_paths op ON op.id = ranked.path_id
JOIN rev.entity_types et ON et.id = op.entity_type_id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
    FROM rev.deprecation_events de
    WHERE de.versioned_object_id = ranked.id
    ORDER BY de.created_at DESC
    LIMIT 1
) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
WHERE ranked.rn = 1
ORDER BY ranked.id;
