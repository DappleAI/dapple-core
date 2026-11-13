-- Select latest brief group version for each name, with content_hash for change detection.
-- Used by guide re-ingest to detect changed content and decide insert vs patch vs reuse.
WITH args AS (
    SELECT * FROM UNNEST($1::TEXT[])
    WITH ORDINALITY AS o(name, ord)
),
latest_versions AS (
    SELECT DISTINCT ON (bgpd.name)
        bg.id,
        op.id AS path_id,
        bgpd.name,
        vo.content_hash
    FROM rev.brief_group_path_details bgpd
    JOIN rev.object_paths op ON op.id = bgpd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.brief_groups bg ON bg.versioned_object_id = vo.id
    WHERE bgpd.name = ANY($1::TEXT[])
    ORDER BY bgpd.name, vo.created_at DESC
)
SELECT
    lv.id,
    lv.path_id,
    lv.name,
    lv.content_hash
FROM latest_versions lv
JOIN args ON args.name = lv.name
ORDER BY args.ord;
