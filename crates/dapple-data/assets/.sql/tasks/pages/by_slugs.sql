args AS (
    SELECT *
    FROM UNNEST($3::TEXT[]) WITH ORDINALITY AS o(slug, ord)
),
task_page AS (
    SELECT t.id, args.ord
    FROM args
    JOIN rev.task_path_details tpd ON tpd.slug = args.slug
    JOIN rev.object_paths op ON op.id = tpd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.tasks t ON t.versioned_object_id = vo.id
    ORDER BY args.ord
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)
