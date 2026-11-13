WITH args AS (
    SELECT *
    FROM UNNEST($3::UUID[]) WITH ORDINALITY AS o(path_id, ord)
),
guide_page AS (
    SELECT g.id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.guides g ON g.versioned_object_id = vo.id
    ORDER BY g.id DESC
    LIMIT $1::BIGINT OFFSET $2::BIGINT
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
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
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

