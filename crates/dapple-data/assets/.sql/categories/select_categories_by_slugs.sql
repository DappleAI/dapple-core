WITH args AS (
    SELECT *
    FROM UNNEST($3::TEXT[]) WITH ORDINALITY AS o(slug, ord)
),
category_page AS (
    SELECT c.id, args.ord
    FROM args
    JOIN rev.category_path_details cpd ON cpd.slug = args.slug
    JOIN rev.object_paths op ON op.id = cpd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.categories c ON c.versioned_object_id = vo.id
    ORDER BY args.ord
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)

SELECT
    c.id,
    c.versioned_object_id,
    vo.path_id,
    cpd.name,
    cpd.slug,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    c.description,
    c.created_at
FROM category_page ckp
JOIN rev.categories c ON c.id = ckp.id
JOIN rev.versioned_objects vo ON vo.id = c.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.category_path_details cpd ON cpd.path_id = op.id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
ORDER BY c.id;

