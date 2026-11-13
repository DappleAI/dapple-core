WITH args AS (
    SELECT *
    FROM UNNEST($2::UUID[]) WITH ORDINALITY AS o(id, ord)
),
task_page AS (
    SELECT t.id, args.ord
    FROM args
    JOIN rev.tasks t ON t.id = args.id
    ORDER BY args.ord
    LIMIT $1::BIGINT OFFSET $3::BIGINT
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
FROM task_page tkp
JOIN rev.tasks t ON t.id = tkp.id
JOIN rev.versioned_objects vo ON vo.id = t.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.task_path_details tpd ON tpd.path_id = op.id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.task_prompts tp ON tp.task_id = t.id
LEFT JOIN rev.prompts p ON p.id = tp.prompt_id
LEFT JOIN rev.versioned_objects vo_p ON vo_p.id = p.versioned_object_id
LEFT JOIN rev.object_paths op_p ON op_p.id = vo_p.path_id
LEFT JOIN rev.prompt_path_details ppd ON ppd.path_id = op_p.id
ORDER BY t.id, tp.id;

