-- Select latest value slot version for each name, with content_hash for change detection.
-- Used by guide re-ingest to detect changed content and decide insert vs patch vs reuse.
WITH args AS (
    SELECT * FROM UNNEST($1::TEXT[])
    WITH ORDINALITY AS o(name, ord)
),
latest_versions AS (
    SELECT DISTINCT ON (vspd.name)
        vs.id,
        op.id AS path_id,
        vspd.name,
        vo.content_hash
    FROM rev.value_slot_path_details vspd
    JOIN rev.object_paths op ON op.id = vspd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.value_slots vs ON vs.versioned_object_id = vo.id
    WHERE vspd.name = ANY($1::TEXT[])
    ORDER BY vspd.name, vo.created_at DESC
)
SELECT
    lv.id,
    lv.path_id,
    lv.name,
    lv.content_hash
FROM latest_versions lv
JOIN args ON args.name = lv.name
ORDER BY args.ord;
