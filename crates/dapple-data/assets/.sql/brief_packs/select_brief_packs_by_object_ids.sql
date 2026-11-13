WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
),
brief_pack_page AS (
    SELECT bp.id
    FROM args
    JOIN rev.brief_packs bp ON bp.id = args.id
)

SELECT
    bp.id,
    bp.versioned_object_id,
    op.id AS path_id,
    bppd.name,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    COALESCE(rev.fluent_body(bp.title_fluent_message_id, 'en'), bp.title, bppd.name) AS "title!",
    COALESCE(rev.fluent_body(bp.description_fluent_message_id, 'en'), bp.description) AS description,
    bp.pack_spec,
    bp.created_at,
        bpg.id as brief_pack_group_id,
    bg.id as group_id,
    bpg.ui_overrides as group_ui_overrides
FROM brief_pack_page bpp
JOIN rev.brief_packs bp ON bp.id = bpp.id
JOIN rev.versioned_objects vo ON vo.id = bp.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.brief_pack_path_details bppd ON bppd.path_id = op.id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.brief_pack_groups bpg ON bpg.brief_pack_id = bp.id
LEFT JOIN rev.brief_groups bg ON bg.id = bpg.brief_group_id
ORDER BY bp.id, bg.id;

