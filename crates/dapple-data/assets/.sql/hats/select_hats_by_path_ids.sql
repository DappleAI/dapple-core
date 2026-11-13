WITH input AS (
    SELECT hc.hat_id,
           array_agg(hc.category_id ORDER BY hc.id) AS category_ids
    FROM rev.hat_categories hc
    JOIN rev.categories c ON hc.category_id = c.id
    GROUP BY hc.hat_id
),
args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(path_id, ord)
),
latest_versions AS (
    SELECT DISTINCT ON (op.id)
        op.id AS path_id,
        h.id AS hat_id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.hats h ON h.versioned_object_id = vo.id
    ORDER BY op.id, vo.created_at DESC
),
hat_page AS (
    SELECT lv.hat_id AS id
    FROM latest_versions lv
)

SELECT
    h.id,
    h.versioned_object_id,
    vo.path_id,
    hpd.name,
    hpd.slug,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    h.description,
    h.created_at,
    COALESCE(input.category_ids, '{}'::uuid[]) AS category_ids
FROM hat_page hp
JOIN rev.hats h ON h.id = hp.id
JOIN rev.versioned_objects vo ON vo.id = h.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.hat_path_details hpd ON hpd.path_id = op.id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN input ON h.id = input.hat_id
ORDER BY h.id;

