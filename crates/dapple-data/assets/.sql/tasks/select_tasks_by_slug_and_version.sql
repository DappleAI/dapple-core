WITH args AS (
    SELECT * FROM UNNEST($1::TEXT[], $2::TEXT[])
    WITH ORDINALITY AS o(slug, content_hash, ord)
)
SELECT
    t.id,
    tpd.slug,
    vo.content_hash
FROM args
JOIN rev.task_path_details tpd ON tpd.slug = args.slug
JOIN rev.object_paths op ON op.id = tpd.path_id
JOIN rev.versioned_objects vo ON vo.path_id = op.id
    AND vo.content_hash = args.content_hash
JOIN rev.tasks t ON t.versioned_object_id = vo.id
ORDER BY args.ord;
