-- Select latest inference profile version for each name, with content_hash for change detection.
-- Used by guide re-ingest to detect changed content and decide insert vs patch vs reuse.
WITH args AS (
    SELECT * FROM UNNEST($1::TEXT[])
    WITH ORDINALITY AS o(name, ord)
),
latest_versions AS (
    SELECT DISTINCT ON (ippd.name)
        ip.id,
        op.id AS path_id,
        ippd.name,
        vo.content_hash
    FROM rev.inference_profile_path_details ippd
    JOIN rev.object_paths op ON op.id = ippd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.inference_profiles ip ON ip.versioned_object_id = vo.id
    WHERE ippd.name = ANY($1::TEXT[])
    ORDER BY ippd.name, vo.created_at DESC
)
SELECT
    lv.id,
    lv.path_id,
    lv.name,
    lv.content_hash
FROM latest_versions lv
JOIN args ON args.name = lv.name
ORDER BY args.ord;
