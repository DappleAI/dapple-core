-- External-API guide listing: scoped to the caller's company namespace and
-- to scope = 'external'. Used by GET /api/guides on external-api.dapple.x.
-- Shared body template for guide SELECT queries.
-- Callers provide a @page slot defining one or more CTEs
-- that produce a `guide_page` CTE with at minimum `guide_page.id`.
-- Fluent-native: no locale column, blocks may have fluent_message_id.
--
-- LIMIT/OFFSET apply to distinct guides (not flat rows) via a page CTE.
WITH latest_deprecation AS (
    SELECT DISTINCT ON (versioned_object_id)
        versioned_object_id,
        to_level_id
    FROM rev.deprecation_events
    ORDER BY versioned_object_id, created_at DESC
),

-- External-API page CTE: list guides scoped to the caller's company namespace.
--
-- Customers do not interact with rev.canonical_release_pins (pinning is a
-- dapple-team release-management concern, not a customer one), so this query
-- does not consult the pin table. Each path's "current" version is simply the
-- latest non-deprecated `versioned_objects` row.
--
-- The customer's surface filters by `(scope = 'external', namespace_company_id
-- = caller's UUID)` at the object_paths layer. There is no pin layer for
-- external content; if and when customer-side pinning ever becomes a real
-- requirement, design it then with a customer-friendly model.
guide_latest AS (
    SELECT DISTINCT ON (op.id)
        g.id
    FROM rev.guides g
    JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
    LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
    LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
    WHERE dl.id IS NULL
      AND op.namespace_company_id = $1::UUID
      AND op.scope = 'external'
    ORDER BY op.id, vo.created_at DESC
),
guide_page AS (
    SELECT id FROM guide_latest
    ORDER BY id
    LIMIT $2::BIGINT OFFSET $3::BIGINT
)

SELECT
    g.id,
    g.versioned_object_id,
    vo.path_id,
    gpd.title,
    gpd.slug,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    g.description,
    g.author,
    g.classification,
    g.source_path,
    g.created_at,
    gc.id AS "chapter_id?",
    gc.parent_chapter_id AS "chapter_parent_chapter_id?",
    gc.title AS "chapter_title?",
    gc.subtitle AS "chapter_subtitle?",
    gc.source_file AS "chapter_source_file?",
    gc.created_at AS "chapter_created_at?",
    gb.id AS "block_id?",
    bdt.key AS "block_directive_type_key?",
    gb.name AS "block_name?",
    gb.content AS "block_content?",
    gb.attributes AS "block_attributes?",
    gb.fluent_message_id AS "block_fluent_message_id?",
    gb.created_at AS "block_created_at?",
    ARRAY_REMOVE(ARRAY_AGG(DISTINCT gt.key), NULL) AS "block_tag_keys?"
FROM guide_page gp
JOIN rev.guides g ON g.id = gp.id
JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.guide_chapters gc ON gc.guide_id = g.id
LEFT JOIN rev.guide_blocks gb ON gb.chapter_id = gc.id
LEFT JOIN rev.guide_block_directive_types bdt ON bdt.id = gb.directive_type_id
LEFT JOIN rev.guide_block_tags gbt ON gbt.block_id = gb.id
LEFT JOIN rev.guide_tags gt ON gt.id = gbt.tag_id
GROUP BY
    g.id, g.versioned_object_id, vo.path_id,
    gpd.title, gpd.slug, vo.content_hash,
    dl.key, g.description, g.author, g.classification, g.source_path,
    g.created_at,
    gc.id, gc.parent_chapter_id, gc.title, gc.subtitle,
    gc.source_file, gc.created_at,
    gb.id, bdt.key, gb.name, gb.content, gb.attributes,
    gb.fluent_message_id,
    gb.created_at
ORDER BY g.id, gc.id, gb.id;

