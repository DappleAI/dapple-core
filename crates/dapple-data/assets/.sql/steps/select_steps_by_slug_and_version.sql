WITH args AS (
    SELECT * FROM UNNEST($1::TEXT[], $2::TEXT[])
    WITH ORDINALITY AS o(slug, content_hash, ord)
)
SELECT
    s.id,
    spd.slug,
    vo.content_hash
FROM args
JOIN rev.step_path_details spd ON spd.slug = args.slug
JOIN rev.object_paths op ON op.id = spd.path_id
JOIN rev.versioned_objects vo ON vo.path_id = op.id
    AND vo.content_hash = args.content_hash
JOIN rev.steps s ON s.versioned_object_id = vo.id
ORDER BY args.ord;
