-- Patch fluent messages using unified versioning (create new version with caller-supplied content_hash)
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::UUID[]
    ) WITH ORDINALITY AS o(content_hash, id, ord)
),
-- Look up existing messages and their versioned objects
existing_messages AS (
    SELECT
        vo.path_id,
        args.id as old_id,
        args.content_hash,
        args.ord
    FROM args
    JOIN rev.fluent_messages fm ON fm.id = args.id
    JOIN rev.versioned_objects vo ON vo.id = fm.versioned_object_id
),
-- Create new versioned objects with caller-supplied content_hash
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        em.path_id,
        em.content_hash
    FROM existing_messages em
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
-- Create new fluent messages
new_messages AS (
    INSERT INTO rev.fluent_messages (versioned_object_id)
    SELECT nv.id
    FROM existing_messages em
    JOIN new_versions nv ON nv.path_id = em.path_id
    RETURNING id, versioned_object_id, created_at
)
SELECT
    nm.id,
    nm.versioned_object_id,
    op.id as path_id,
    fmpd.name,
    fmpd.description,
    nv.content_hash,
    nm.created_at
FROM new_messages nm
JOIN new_versions nv ON nv.id = nm.versioned_object_id
JOIN rev.object_paths op ON op.id = nv.path_id
JOIN rev.fluent_message_path_details fmpd ON fmpd.path_id = op.id
JOIN existing_messages em ON em.path_id = nv.path_id
ORDER BY nm.id;
