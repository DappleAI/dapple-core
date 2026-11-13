WITH args AS (
    SELECT *
    FROM UNNEST($2::UUID[]) WITH ORDINALITY AS o(id, ord)
),
step_page AS (
    SELECT s.id
    FROM args
    JOIN rev.steps s ON s.id = args.id
    ORDER BY s.id
    LIMIT $1::BIGINT OFFSET $3::BIGINT
)

SELECT
    s.id,
    s.versioned_object_id,
    vo.path_id,
    spd.name,
    spd.slug,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    COALESCE(rev.fluent_body(s.description_fluent_message_id, 'en'), s.description, spd.name) AS "description!",
    s.created_at,
    st.id AS "step_task_id?",
    st.created_at AS "step_task_created_at?",
    t.id AS "step_task_task_id?"
FROM step_page sp
JOIN rev.steps s ON s.id = sp.id
JOIN rev.versioned_objects vo ON vo.id = s.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.step_path_details spd ON spd.path_id = op.id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.step_tasks st ON st.step_id = s.id
LEFT JOIN rev.tasks t ON t.id = st.task_id
ORDER BY s.id, st.ord NULLS FIRST;

