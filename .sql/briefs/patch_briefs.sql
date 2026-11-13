-- Patch briefs using unified versioning (create new version with caller-supplied content_hash)
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::UUID[],
        $4::TEXT[]
    ) WITH ORDINALITY AS o(content_hash, description, id, title, ord)
),
existing_briefs AS (
    SELECT
        vo.path_id,
        args.id as old_id,
        args.title,
        args.description,
        args.content_hash,
        args.ord
    FROM args
    JOIN rev.briefs b ON b.id = args.id
    JOIN rev.versioned_objects vo ON vo.id = b.versioned_object_id
),
-- Create new versioned objects with caller-supplied content_hash
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        eb.path_id,
        eb.content_hash
    FROM existing_briefs eb
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
-- Create new briefs
new_briefs AS (
    INSERT INTO rev.briefs (versioned_object_id, title, description)
    SELECT
        nv.id,
        eb.title,
        eb.description
    FROM existing_briefs eb
    JOIN new_versions nv ON nv.path_id = eb.path_id
    RETURNING id, versioned_object_id, title, description, created_at
)
SELECT
    nb.id,
    nb.versioned_object_id,
    op.id as path_id,
    bpd.name,
    nv.content_hash,
    nb.title,
    nb.description,
    nb.created_at,
    (SELECT b.id FROM rev.briefs b WHERE b.id = eb.old_id) AS old_brief_id
FROM new_briefs nb
JOIN new_versions nv ON nv.id = nb.versioned_object_id
JOIN rev.object_paths op ON op.id = nv.path_id
JOIN rev.brief_path_details bpd ON bpd.path_id = op.id
JOIN existing_briefs eb ON eb.path_id = nv.path_id
ORDER BY nb.id;
