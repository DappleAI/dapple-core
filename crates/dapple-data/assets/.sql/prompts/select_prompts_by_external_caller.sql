WITH prompt_latest AS (
    SELECT DISTINCT ON (op.id)
        p.id
    FROM rev.prompts p
    JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
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
prompt_page AS (
    SELECT id FROM prompt_latest
    ORDER BY id
    LIMIT $2::BIGINT OFFSET $3::BIGINT
)

SELECT
    p.id,
    p.versioned_object_id,
    op.id AS path_id,
    ppd.name,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    p.description,
    p.created_at,
                    pfm.id as "prompt_fluent_message_id?",
    pfm.fluent_message_id as "fluent_message_id?",
    pfm.role as fluent_message_role,
        ROW_NUMBER() OVER (PARTITION BY p.id ORDER BY pfm.id) - 1 as "fluent_message_ordinal?"
FROM prompt_page prp
JOIN rev.prompts p ON p.id = prp.id
JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.prompt_path_details ppd ON ppd.path_id = op.id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.prompt_fluent_messages pfm ON pfm.prompt_id = p.id
ORDER BY p.id, pfm.id;

