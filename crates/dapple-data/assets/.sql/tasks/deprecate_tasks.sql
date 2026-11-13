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
    t.id,
    t.versioned_object_id,
    vo.path_id,
    tpd.name,
    tpd.slug,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    t.description,
    t.function,
    t.function_input,
    t.created_at,
    tp.id AS "task_prompt_id?",
    p.id AS "prompt_id?",
    ppd.name AS "prompt_name?"
FROM resolved
JOIN rev.versioned_objects vo ON vo.id = resolved.versioned_object_id
JOIN rev.tasks t ON t.versioned_object_id = vo.id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.task_path_details tpd ON tpd.path_id = op.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = $4::SMALLINT
LEFT JOIN rev.task_prompts tp ON tp.task_id = t.id
LEFT JOIN rev.prompts p ON p.id = tp.prompt_id
LEFT JOIN rev.versioned_objects vo_p ON vo_p.id = p.versioned_object_id
LEFT JOIN rev.object_paths op_p ON op_p.id = vo_p.path_id
LEFT JOIN rev.prompt_path_details ppd ON ppd.path_id = op_p.id
ORDER BY t.id, tp.id;
