WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(path_id, ord)
),
asset_page AS (
    SELECT a.id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.assets a ON a.versioned_object_id = vo.id
)

SELECT
    a.id,
    a.versioned_object_id,
    vo.path_id,
    ak.key as asset_kind_key,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    a.draft_id,
    a.content_length,
    a.mime_type,
    a.created_at
FROM asset_page akp
JOIN rev.assets a ON a.id = akp.id
JOIN rev.versioned_objects vo ON vo.id = a.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.asset_path_details apd ON apd.path_id = op.id
JOIN rev.asset_kinds ak ON ak.id = apd.asset_kind_id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.drafts d ON d.id = a.draft_id
ORDER BY a.id;

