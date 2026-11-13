-- Select playbooks by slug + exact content_hash for dedup checks
WITH args AS (
    SELECT * FROM UNNEST($1::TEXT[], $2::TEXT[])
    WITH ORDINALITY AS o(slug, content_hash, ord)
)
SELECT
    p.id,
    ppd.slug,
    vo.content_hash
FROM args
JOIN rev.playbook_path_details ppd ON ppd.slug = args.slug
JOIN rev.object_paths op ON op.id = ppd.path_id
JOIN rev.versioned_objects vo ON vo.path_id = op.id
    AND vo.content_hash = args.content_hash
JOIN rev.playbooks p ON p.versioned_object_id = vo.id
ORDER BY args.ord;
