-- Select brief groups by path IDs using unified versioning (returns all versions)
-- Shared body template for brief group SELECT queries.
-- Callers provide a @page slot defining one or more CTEs
-- that produce a `brief_group_page` CTE with at minimum `brief_group_page.id`.
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
brief_group_page AS (
    SELECT bg.id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.brief_groups bg ON bg.versioned_object_id = vo.id
)

SELECT
    bg.id,
    bg.versioned_object_id,
    op.id AS path_id,
    bgpd.name,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    bg.title,
    bg.description,
    bg.created_at,
    -- Brief association fields (nullable due to LEFT JOIN)
    bgb.id as brief_group_brief_id,
    b.id as brief_id
FROM brief_group_page bgp
JOIN rev.brief_groups bg ON bg.id = bgp.id
JOIN rev.versioned_objects vo ON vo.id = bg.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.brief_group_path_details bgpd ON bgpd.path_id = op.id
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.brief_group_briefs bgb ON bgb.brief_group_id = bg.id
LEFT JOIN rev.briefs b ON b.id = bgb.brief_id
ORDER BY bg.id, b.id;

