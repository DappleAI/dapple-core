-- Select all brief packs using unified versioning
--
-- LIMIT/OFFSET apply to distinct brief packs (not flat rows) via a page CTE.
-- The flat join (brief_pack x brief_pack_groups x brief_groups) produces many rows
-- per brief pack, so limiting flat rows would silently truncate results.
-- Shared body template for brief pack SELECT queries.
-- Callers provide a @page slot defining one or more CTEs
-- that produce a `brief_pack_page` CTE with at minimum `brief_pack_page.id`.
WITH latest_deprecation AS (
    SELECT DISTINCT ON (versioned_object_id)
        versioned_object_id,
        to_level_id
    FROM rev.deprecation_events
    ORDER BY versioned_object_id, created_at DESC
),

brief_pack_page AS (
    SELECT bp.id
    FROM rev.brief_packs bp
    JOIN rev.versioned_objects vo ON vo.id = bp.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.brief_pack_path_details bppd ON bppd.path_id = op.id
    ORDER BY bp.id
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)

SELECT
    bp.id,
    bp.versioned_object_id,
    op.id AS path_id,
    bppd.name,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    bp.title,
    bp.description,
    bp.pack_spec,
    bp.created_at,
    -- Group association fields (nullable due to LEFT JOIN)
    bpg.id as brief_pack_group_id,
    bg.id as group_id,
    bpg.ui_overrides as group_ui_overrides
FROM brief_pack_page bpp
JOIN rev.brief_packs bp ON bp.id = bpp.id
JOIN rev.versioned_objects vo ON vo.id = bp.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.brief_pack_path_details bppd ON bppd.path_id = op.id
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.brief_pack_groups bpg ON bpg.brief_pack_id = bp.id
LEFT JOIN rev.brief_groups bg ON bg.id = bpg.brief_group_id
ORDER BY bp.id, bg.id;

