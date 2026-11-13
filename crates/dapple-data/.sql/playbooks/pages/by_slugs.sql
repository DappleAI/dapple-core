args AS (
    SELECT *
    FROM UNNEST($3::TEXT[]) WITH ORDINALITY AS o(slug, ord)
),
playbook_latest AS (
    SELECT DISTINCT ON (args.slug)
        p.id, args.ord
    FROM args
    JOIN rev.playbook_path_details ppd ON ppd.slug = args.slug
    JOIN rev.object_paths op ON op.id = ppd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.playbooks p ON p.versioned_object_id = vo.id
    ORDER BY args.slug, vo.created_at DESC
),
playbook_page AS (
    SELECT id
    FROM playbook_latest
    ORDER BY ord
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)
