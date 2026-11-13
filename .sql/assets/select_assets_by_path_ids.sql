-- Select assets by path IDs using unified versioning (returns all versions for each path)
-- Shared body template for asset SELECT queries.
-- Callers provide a @page slot defining one or more CTEs
-- that produce an `asset_page` CTE with at minimum `asset_page.id`.
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
asset_page AS (
    SELECT a.id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.assets a ON a.versioned_object_id = vo.id
)

SELECT
    a.id,
    a.versioned_object_id,
    vo.path_id,
    ak.key as asset_kind_key,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    a.draft_id,
    a.content_length,
    a.mime_type,
    a.created_at
FROM asset_page akp
JOIN rev.assets a ON a.id = akp.id
JOIN rev.versioned_objects vo ON vo.id = a.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.asset_path_details apd ON apd.path_id = op.id
JOIN rev.asset_kinds ak ON ak.id = apd.asset_kind_id
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.drafts d ON d.id = a.draft_id
ORDER BY a.id;

