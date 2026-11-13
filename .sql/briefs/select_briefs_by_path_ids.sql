-- Select briefs by path IDs using unified versioning (returns all versions)
-- Shared body template for brief SELECT queries.
-- Callers provide a @page slot defining one or more CTEs
-- that produce a `brief_page` CTE with at minimum `brief_page.id`.
WITH latest_deprecation AS (
    SELECT DISTINCT ON (versioned_object_id)
        versioned_object_id,
        to_level_id
    FROM rev.deprecation_events
    ORDER BY versioned_object_id, created_at DESC
),

args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(path_id, ord)
),
brief_page AS (
    SELECT b.id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.briefs b ON b.versioned_object_id = vo.id
)

SELECT
    b.id,
    b.versioned_object_id,
    op.id AS path_id,
    bpd.name,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    b.title,
    b.description,
    b.created_at
FROM brief_page bkp
JOIN rev.briefs b ON b.id = bkp.id
JOIN rev.versioned_objects vo ON vo.id = b.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.brief_path_details bpd ON bpd.path_id = op.id
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
ORDER BY b.id;

