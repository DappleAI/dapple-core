WITH stage_latest AS (
    SELECT DISTINCT ON (op.id)
        st.id
    FROM rev.stages st
    JOIN rev.versioned_objects vo ON vo.id = st.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    LEFT JOIN LATERAL (
        SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

    ) ld ON TRUE
    LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
    WHERE dl.id IS NULL
      AND op.namespace_company_id = $1::UUID
    ORDER BY op.id, vo.created_at DESC
),
stage_page AS (
    SELECT id FROM stage_latest
    ORDER BY id
    LIMIT $2::BIGINT OFFSET $3::BIGINT
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
FROM stage_page stp
JOIN rev.stages st ON st.id = stp.id
JOIN rev.versioned_objects vo ON vo.id = st.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.stage_path_details spd ON spd.path_id = op.id
LEFT JOIN rev.stage_kinds sk ON sk.id = st.kind_id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
ORDER BY st.id;

