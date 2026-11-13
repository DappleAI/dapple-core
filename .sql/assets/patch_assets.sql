-- Patch assets using unified versioning (create new version with caller-supplied content_hash)
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::BIGINT[],
        $3::UUID[],
        $4::UUID[],
        $5::TEXT[]
    ) WITH ORDINALITY AS o(content_hash, content_length, draft_id, id, mime_type, ord)
),
-- Look up existing assets and their versioned objects
existing_assets AS (
    SELECT
        vo.path_id,
        args.id as old_id,
        args.mime_type,
        args.content_length,
        args.draft_id,
        args.content_hash,
        args.ord
    FROM args
    JOIN rev.assets a ON a.id = args.id
    JOIN rev.versioned_objects vo ON vo.id = a.versioned_object_id
),
-- Create new versioned objects with caller-supplied content_hash
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        ea.path_id,
        ea.content_hash
    FROM existing_assets ea
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
-- Create new assets
new_assets AS (
    INSERT INTO rev.assets (versioned_object_id, draft_id, content_length, mime_type)
    SELECT
        nv.id,
        ea.draft_id,
        ea.content_length,
        ea.mime_type
    FROM existing_assets ea
    JOIN new_versions nv ON nv.path_id = ea.path_id
    RETURNING id, versioned_object_id, draft_id, content_length, mime_type, created_at
)
SELECT
    na.id,
    na.versioned_object_id,
    nv.path_id,
    ak.key as asset_kind_key,
    nv.content_hash,
    na.draft_id,
    na.content_length,
    na.mime_type,
    na.created_at
FROM new_assets na
JOIN new_versions nv ON nv.id = na.versioned_object_id
JOIN rev.object_paths op ON op.id = nv.path_id
JOIN rev.asset_path_details apd ON apd.path_id = op.id
JOIN rev.asset_kinds ak ON ak.id = apd.asset_kind_id
JOIN existing_assets ea ON ea.path_id = nv.path_id
LEFT JOIN rev.drafts d ON d.id = na.draft_id
ORDER BY na.id;
