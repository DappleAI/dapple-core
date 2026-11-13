WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(path_id, ord)
),
value_slot_page AS (
    SELECT vs.id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.value_slots vs ON vs.versioned_object_id = vo.id
)

SELECT
    vs.id,
    vs.versioned_object_id,
    op.id AS path_id,
    vspd.name,
    vk.key as value_kind_key,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    vs.title,
    vs.description,
    vs.constraints,
    vs.cardinality,
    vs.min_selections,
    vs.max_selections,
    vs.created_at
FROM value_slot_page vskp
JOIN rev.value_slots vs ON vs.id = vskp.id
JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.value_slot_path_details vspd ON vspd.path_id = op.id
JOIN rev.value_kinds vk ON vk.id = vs.value_kind_id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
ORDER BY vs.id;

