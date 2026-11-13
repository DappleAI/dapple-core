-- Select latest step version for each slug, with path_id for patch routing.
-- Used by guide re-ingest to detect version changes and decide insert vs patch vs reuse.
WITH args AS (
    SELECT * FROM UNNEST($1::TEXT[])
    WITH ORDINALITY AS o(slug, ord)
),
latest_versions AS (
    SELECT DISTINCT ON (spd.slug)
        s.id,
        op.id AS path_id,
        spd.slug,
        vo.content_hash
    FROM rev.step_path_details spd
    JOIN rev.object_paths op ON op.id = spd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.steps s ON s.versioned_object_id = vo.id
    WHERE spd.slug = ANY($1::TEXT[])
    ORDER BY spd.slug, vo.created_at DESC
)
SELECT
    lv.id,
    lv.path_id,
    lv.slug,
    lv.content_hash
FROM latest_versions lv
JOIN args ON args.slug = lv.slug
ORDER BY args.ord;
