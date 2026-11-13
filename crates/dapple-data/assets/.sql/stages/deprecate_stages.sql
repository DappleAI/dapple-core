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
    st.id,
    st.versioned_object_id,
    vo.path_id,
    spd.name,
    spd.slug,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    COALESCE(rev.fluent_body(st.description_fluent_message_id, 'en'), st.description, spd.name) AS "description!",
    sk.key AS "kind?",
    st.icons AS "icons?",
    st.created_at
FROM resolved
JOIN rev.versioned_objects vo ON vo.id = resolved.versioned_object_id
JOIN rev.stages st ON st.versioned_object_id = vo.id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.stage_path_details spd ON spd.path_id = op.id
LEFT JOIN rev.stage_kinds sk ON sk.id = st.kind_id
LEFT JOIN rev.deprecation_levels dl ON dl.id = $4::SMALLINT
ORDER BY st.id;
