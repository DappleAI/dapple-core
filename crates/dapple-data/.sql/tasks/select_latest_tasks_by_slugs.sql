-- Select latest task version for each slug, with function_input for script comparison.
-- Used by guide re-ingest to detect changed scripts and decide insert vs patch.
WITH args AS (
    SELECT * FROM UNNEST($1::TEXT[])
    WITH ORDINALITY AS o(slug, ord)
),
latest_versions AS (
    SELECT DISTINCT ON (tpd.slug)
        t.id,
        op.id AS path_id,
        tpd.slug,
        vo.content_hash,
        t.function_input
    FROM rev.task_path_details tpd
    JOIN rev.object_paths op ON op.id = tpd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.tasks t ON t.versioned_object_id = vo.id
    WHERE tpd.slug = ANY($1::TEXT[])
    ORDER BY tpd.slug, vo.created_at DESC
)
SELECT
    lv.id,
    lv.path_id,
    lv.slug,
    lv.content_hash,
    lv.function_input
FROM latest_versions lv
JOIN args ON args.slug = lv.slug
ORDER BY args.ord;
