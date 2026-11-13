-- Patch brief groups using unified versioning (create new version with caller-supplied content_hash)
-- Note: This creates the new version but NOT the brief associations.
-- You must call insert_brief_group_briefs.sql separately to add briefs.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::UUID[],
        $4::TEXT[]
    ) WITH ORDINALITY AS o(content_hash, description, id, title, ord)
),
existing_groups AS (
    SELECT
        vo.path_id,
        args.id as old_id,
        args.title,
        args.description,
        args.content_hash,
        args.ord
    FROM args
    JOIN rev.brief_groups bg ON bg.id = args.id
    JOIN rev.versioned_objects vo ON vo.id = bg.versioned_object_id
),
-- Create new versioned objects with caller-supplied content_hash
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        eg.path_id,
        eg.content_hash
    FROM existing_groups eg
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
-- Create new brief groups
new_groups AS (
    INSERT INTO rev.brief_groups (versioned_object_id, title, description)
    SELECT
        nv.id,
        eg.title,
        eg.description
    FROM existing_groups eg
    JOIN new_versions nv ON nv.path_id = eg.path_id
    RETURNING id, versioned_object_id, title, description, created_at
)
SELECT
    ng.id,
    ng.versioned_object_id,
    op.id AS path_id,
    bgpd.name,
    nv.content_hash,
    ng.title,
    ng.description,
    ng.created_at
FROM new_groups ng
JOIN new_versions nv ON nv.id = ng.versioned_object_id
JOIN rev.object_paths op ON op.id = nv.path_id
JOIN rev.brief_group_path_details bgpd ON bgpd.path_id = op.id
JOIN existing_groups eg ON eg.path_id = nv.path_id
ORDER BY ng.id;
