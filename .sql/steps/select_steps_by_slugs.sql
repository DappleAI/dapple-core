-- Select steps by slugs using path_details lookup
--
-- LIMIT/OFFSET apply to distinct steps (not flat rows) via a page CTE.
-- Shared body template for step SELECT queries.
-- Callers provide a @page slot defining one or more CTEs
-- that produce a `step_page` CTE with at minimum `step_page.id`.
WITH latest_deprecation AS (
    SELECT DISTINCT ON (versioned_object_id)
        versioned_object_id,
        to_level_id
    FROM rev.deprecation_events
    ORDER BY versioned_object_id, created_at DESC
),

args AS (
    SELECT *
    FROM UNNEST($3::TEXT[]) WITH ORDINALITY AS o(slug, ord)
),
step_page AS (
    SELECT s.id, args.ord
    FROM args
    JOIN rev.step_path_details spd ON spd.slug = args.slug
    JOIN rev.object_paths op ON op.id = spd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.steps s ON s.versioned_object_id = vo.id
    ORDER BY args.ord
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)

SELECT
    s.id,
    s.versioned_object_id,
    vo.path_id,
    spd.name,
    spd.slug,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    s.description,
    s.created_at,
    st.id AS "step_task_id?",
    st.created_at AS "step_task_created_at?",
    t.id AS "step_task_task_id?"
FROM step_page sp
JOIN rev.steps s ON s.id = sp.id
JOIN rev.versioned_objects vo ON vo.id = s.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.step_path_details spd ON spd.path_id = op.id
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.step_tasks st ON st.step_id = s.id
LEFT JOIN rev.tasks t ON st.task_id = t.id
ORDER BY s.id, st.id;

