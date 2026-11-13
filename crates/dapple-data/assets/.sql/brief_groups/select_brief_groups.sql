WITH brief_group_page AS (
    SELECT bg.id
    FROM rev.brief_groups bg
    JOIN rev.versioned_objects vo ON vo.id = bg.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.brief_group_path_details bgpd ON bgpd.path_id = op.id
    ORDER BY bg.id
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)

SELECT
    bg.id,
    bg.versioned_object_id,
    op.id AS path_id,
    bgpd.name,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    COALESCE(rev.fluent_body(bg.title_fluent_message_id, 'en'), bg.title, bgpd.name) AS "title!",
    COALESCE(rev.fluent_body(bg.description_fluent_message_id, 'en'), bg.description) AS description,
    bg.created_at,
        bgb.id as brief_group_brief_id,
    b.id as brief_id
FROM brief_group_page bgp
JOIN rev.brief_groups bg ON bg.id = bgp.id
JOIN rev.versioned_objects vo ON vo.id = bg.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.brief_group_path_details bgpd ON bgpd.path_id = op.id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.brief_group_briefs bgb ON bgb.brief_group_id = bg.id
LEFT JOIN rev.briefs b ON b.id = bgb.brief_id
ORDER BY bg.id, b.id;

