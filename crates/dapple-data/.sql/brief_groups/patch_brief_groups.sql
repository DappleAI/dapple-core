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
new_versions_inserted AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        eg.path_id,
        eg.content_hash
    FROM existing_groups eg
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
-- Include already-existing versioned objects (conflict hit — content reverted)
all_versions AS (
    SELECT nvi.id, nvi.path_id, nvi.content_hash, nvi.created_at
    FROM new_versions_inserted nvi
    UNION ALL
    SELECT vo.id, vo.path_id, vo.content_hash, vo.created_at
    FROM existing_groups eg
    JOIN rev.versioned_objects vo ON vo.path_id = eg.path_id AND vo.content_hash = eg.content_hash
    WHERE NOT EXISTS (
        SELECT 1 FROM new_versions_inserted nvi
        WHERE nvi.path_id = eg.path_id AND nvi.content_hash = eg.content_hash
    )
),
-- Create new brief groups (skip if entity row already exists for this versioned_object)
new_groups AS (
    INSERT INTO rev.brief_groups (versioned_object_id, title, description)
    SELECT
        av.id,
        eg.title,
        eg.description
    FROM existing_groups eg
    JOIN all_versions av ON av.path_id = eg.path_id
    WHERE NOT EXISTS (SELECT 1 FROM rev.brief_groups bg WHERE bg.versioned_object_id = av.id)
    RETURNING id, versioned_object_id, title, description, created_at
),
-- Include already-existing brief groups (for reverted versions)
all_groups AS (
    SELECT ng.id, ng.versioned_object_id, ng.title, ng.description, ng.created_at
    FROM new_groups ng
    UNION ALL
    SELECT bg.id, bg.versioned_object_id, bg.title, bg.description, bg.created_at
    FROM rev.brief_groups bg
    JOIN all_versions av ON bg.versioned_object_id = av.id
    WHERE NOT EXISTS (SELECT 1 FROM new_groups ng WHERE ng.versioned_object_id = av.id)
)
SELECT
    ag.id AS "id!",
    ag.versioned_object_id AS "versioned_object_id!",
    op.id AS "path_id!",
    bgpd.name AS "name!",
    av.content_hash AS "content_hash!",
    ag.title AS "title!",
    ag.description,
    ag.created_at AS "created_at!"
FROM all_groups ag
JOIN all_versions av ON av.id = ag.versioned_object_id
JOIN rev.object_paths op ON op.id = av.path_id
JOIN rev.brief_group_path_details bgpd ON bgpd.path_id = op.id
JOIN existing_groups eg ON eg.path_id = av.path_id
ORDER BY ag.id;
