WITH args AS (
    SELECT *
    FROM UNNEST(
        $2::UUID[],
        $3::TEXT[]
    ) WITH ORDINALITY AS o(
        path_id,
        reason,
        ord
    )
),
latest_versions AS (
    SELECT DISTINCT ON (op.id)
        op.id AS path_id,
        vo.id AS versioned_object_id,
        ld.to_level_id AS from_level_id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    LEFT JOIN LATERAL (
        SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

    ) ld ON TRUE
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
FROM resolved
JOIN rev.versioned_objects vo ON vo.id = resolved.versioned_object_id
JOIN rev.value_slots vs ON vs.versioned_object_id = vo.id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.value_slot_path_details vspd ON vspd.path_id = op.id
JOIN rev.value_kinds vk ON vk.id = vs.value_kind_id
LEFT JOIN rev.deprecation_levels dl ON dl.id = $4::SMALLINT
ORDER BY vs.id;
