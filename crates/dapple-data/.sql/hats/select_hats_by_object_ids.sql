-- Select hats by object IDs using unified versioning
-- Shared body template for hat SELECT queries.
-- Callers provide a @page slot defining one or more CTEs
-- that produce a `hat_page` CTE with at minimum `hat_page.id`.
WITH latest_deprecation AS (
    SELECT DISTINCT ON (versioned_object_id)
        versioned_object_id,
        to_level_id
    FROM rev.deprecation_events
    ORDER BY versioned_object_id, created_at DESC
),

input AS (
    SELECT hc.hat_id,
           array_agg(hc.category_id ORDER BY hc.id) AS category_ids
    FROM rev.hat_categories hc
    JOIN rev.categories c ON hc.category_id = c.id
    GROUP BY hc.hat_id
),
args AS (
    SELECT *
    FROM UNNEST($2::UUID[]) WITH ORDINALITY AS o(id, ord)
),
hat_page AS (
    SELECT h.id
    FROM args
    JOIN rev.hats h ON h.id = args.id
    ORDER BY args.ord
    LIMIT $1::BIGINT OFFSET $3::BIGINT
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
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN input ON h.id = input.hat_id
ORDER BY h.id;

