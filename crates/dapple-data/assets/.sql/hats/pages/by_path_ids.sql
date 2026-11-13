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
