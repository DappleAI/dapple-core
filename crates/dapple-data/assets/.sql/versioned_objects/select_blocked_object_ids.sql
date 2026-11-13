WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    vo.id,
            dl.key AS "deprecation_level_key!"
FROM args
JOIN rev.versioned_objects vo ON vo.id = args.id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
WHERE dl.key IN ('retired', 'quarantined')
ORDER BY vo.id;
