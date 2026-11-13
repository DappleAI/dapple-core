WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
),
brief_page AS (
    SELECT b.id
    FROM args
    JOIN rev.briefs b ON b.id = args.id
)

SELECT
    b.id,
    b.versioned_object_id,
    op.id AS path_id,
    bpd.name,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    COALESCE(rev.fluent_body(b.title_fluent_message_id, 'en'), b.title, bpd.name) AS "title!",
    COALESCE(rev.fluent_body(b.description_fluent_message_id, 'en'), b.description) AS description,
    b.created_at
FROM brief_page bkp
JOIN rev.briefs b ON b.id = bkp.id
JOIN rev.versioned_objects vo ON vo.id = b.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.brief_path_details bpd ON bpd.path_id = op.id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
ORDER BY b.id;

