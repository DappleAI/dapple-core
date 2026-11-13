-- Patch brief packs using unified versioning (create new version with caller-supplied content_hash)
-- Note: This creates the new version but NOT the group associations.
-- You must call insert_brief_pack_groups.sql separately to add groups.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::UUID[],
        $4::JSONB[],
        $5::TEXT[]
    ) WITH ORDINALITY AS o(content_hash, description, id, pack_spec, title, ord)
),
existing_packs AS (
    SELECT
        vo.path_id,
        args.id as old_id,
        args.title,
        args.description,
        args.pack_spec,
        args.content_hash,
        args.ord
    FROM args
    JOIN rev.brief_packs bp ON bp.id = args.id
    JOIN rev.versioned_objects vo ON vo.id = bp.versioned_object_id
),
-- Create new versioned objects with caller-supplied content_hash
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        ep.path_id,
        ep.content_hash
    FROM existing_packs ep
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
-- Create new brief packs
new_packs AS (
    INSERT INTO rev.brief_packs (versioned_object_id, title, description, pack_spec)
    SELECT
        nv.id,
        ep.title,
        ep.description,
        COALESCE(ep.pack_spec, '{}'::jsonb)
    FROM existing_packs ep
    JOIN new_versions nv ON nv.path_id = ep.path_id
    RETURNING id, versioned_object_id, title, description, pack_spec, created_at
)
SELECT
    np.id,
    np.versioned_object_id,
    op.id AS path_id,
    bppd.name,
    nv.content_hash,
    np.title,
    np.description,
    np.pack_spec,
    np.created_at
FROM new_packs np
JOIN new_versions nv ON nv.id = np.versioned_object_id
JOIN rev.object_paths op ON op.id = nv.path_id
JOIN rev.brief_pack_path_details bppd ON bppd.path_id = op.id
JOIN existing_packs ep ON ep.path_id = nv.path_id
ORDER BY np.id;
