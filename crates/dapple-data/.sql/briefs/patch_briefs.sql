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
new_versions_inserted AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        eb.path_id,
        eb.content_hash
    FROM existing_briefs eb
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
-- Include already-existing versioned objects (conflict hit — content reverted)
all_versions AS (
    SELECT nvi.id, nvi.path_id, nvi.content_hash, nvi.created_at
    FROM new_versions_inserted nvi
    UNION ALL
    SELECT vo.id, vo.path_id, vo.content_hash, vo.created_at
    FROM existing_briefs eb
    JOIN rev.versioned_objects vo ON vo.path_id = eb.path_id AND vo.content_hash = eb.content_hash
    WHERE NOT EXISTS (
        SELECT 1 FROM new_versions_inserted nvi
        WHERE nvi.path_id = eb.path_id AND nvi.content_hash = eb.content_hash
    )
),
-- Create new briefs (skip if entity row already exists for this versioned_object)
new_briefs AS (
    INSERT INTO rev.briefs (versioned_object_id, title, description)
    SELECT
        av.id,
        eb.title,
        eb.description
    FROM existing_briefs eb
    JOIN all_versions av ON av.path_id = eb.path_id
    WHERE NOT EXISTS (SELECT 1 FROM rev.briefs b WHERE b.versioned_object_id = av.id)
    RETURNING id, versioned_object_id, title, description, created_at
),
-- Include already-existing briefs (for reverted versions)
all_briefs AS (
    SELECT nb.id, nb.versioned_object_id, nb.title, nb.description, nb.created_at
    FROM new_briefs nb
    UNION ALL
    SELECT b.id, b.versioned_object_id, b.title, b.description, b.created_at
    FROM rev.briefs b
    JOIN all_versions av ON b.versioned_object_id = av.id
    WHERE NOT EXISTS (SELECT 1 FROM new_briefs nb WHERE nb.versioned_object_id = av.id)
)
SELECT
    ab.id AS "id!",
    ab.versioned_object_id AS "versioned_object_id!",
    op.id AS "path_id!",
    bpd.name AS "name!",
    av.content_hash AS "content_hash!",
    ab.title AS "title!",
    ab.description,
    ab.created_at AS "created_at!",
    (SELECT b.id FROM rev.briefs b WHERE b.id = eb.old_id) AS old_brief_id
FROM all_briefs ab
JOIN all_versions av ON av.id = ab.versioned_object_id
JOIN rev.object_paths op ON op.id = av.path_id
JOIN rev.brief_path_details bpd ON bpd.path_id = op.id
JOIN existing_briefs eb ON eb.path_id = av.path_id
ORDER BY ab.id;
