-- Finalize a draft into an asset
-- Atomically: copy chunk refs from draft_chunks to asset_chunks, create asset
-- Note: chunks already exist in rev.chunks (created at draft write time)
WITH draft_info AS (
    SELECT d.id, d.asset_kind_id, d.mime_type
    FROM rev.drafts d
    WHERE d.id = $1
),
-- Get draft chunk refs with chunk sizes (chunks already exist)
draft_chunk_refs AS (
    SELECT
        dc.chunk_id,
        dc.ordinal,
        octet_length(c.data) as chunk_size
    FROM rev.draft_chunks dc
    JOIN draft_info di ON di.id = dc.draft_id
    JOIN rev.chunks c ON c.id = dc.chunk_id
    ORDER BY dc.ordinal
),
-- Calculate total content length from chunks
content_stats AS (
    SELECT
        COUNT(*)::BIGINT as chunk_count,
        COALESCE(SUM(chunk_size), 0)::BIGINT as total_length
    FROM draft_chunk_refs
),
-- Get the entity type ID for assets
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'asset'
),
-- Create object path
new_path AS (
    INSERT INTO rev.object_paths (entity_type_id)
    SELECT id FROM entity_type
    RETURNING id
),
-- Create path details
new_path_details AS (
    INSERT INTO rev.asset_path_details (path_id, asset_kind_id)
    SELECT np.id, di.asset_kind_id
    FROM new_path np, draft_info di
    RETURNING path_id
),
-- Create versioned object
new_version AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT np.id, $2::TEXT
    FROM new_path np
    RETURNING id, path_id, content_hash, created_at
),
-- Create asset
new_asset AS (
    INSERT INTO rev.assets (versioned_object_id, draft_id, content_length, mime_type)
    SELECT
        nv.id,
        di.id,
        cs.total_length,
        di.mime_type
    FROM new_version nv, draft_info di, content_stats cs
    RETURNING id, versioned_object_id, draft_id, content_length, mime_type, created_at
),
-- Link asset to existing chunks (copy refs from draft_chunks)
new_links AS (
    INSERT INTO rev.asset_chunks (asset_id, chunk_id, ordinal)
    SELECT na.id, dcr.chunk_id, dcr.ordinal
    FROM new_asset na
    CROSS JOIN draft_chunk_refs dcr
    RETURNING asset_id, chunk_id
)
SELECT
    na.id as asset_id,
    nv.path_id,
    na.draft_id,
    na.content_length,
    na.mime_type,
    cs.chunk_count,
    nv.content_hash,
    na.created_at
FROM new_asset na
JOIN new_version nv ON nv.id = na.versioned_object_id
JOIN new_path np ON np.id = nv.path_id
JOIN draft_info di ON di.id = na.draft_id
CROSS JOIN content_stats cs;
