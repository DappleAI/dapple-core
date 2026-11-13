-- Select stages by slugs using path_details lookup
-- Shared body template for stage SELECT queries.
-- Callers provide a @page slot defining one or more CTEs
-- that produce a `stage_page` CTE with at minimum `stage_page.id`.
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
stage_page AS (
    SELECT st.id, args.ord
    FROM args
    JOIN rev.stage_path_details spd ON spd.slug = args.slug
    JOIN rev.object_paths op ON op.id = spd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.stages st ON st.versioned_object_id = vo.id
    ORDER BY args.ord
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)

SELECT
    st.id,
    st.versioned_object_id,
    vo.path_id,
    spd.name,
    spd.slug,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    st.description,
    st.created_at
FROM stage_page stp
JOIN rev.stages st ON st.id = stp.id
JOIN rev.versioned_objects vo ON vo.id = st.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.stage_path_details spd ON spd.path_id = op.id
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
ORDER BY st.id;

