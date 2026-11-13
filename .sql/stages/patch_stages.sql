-- Create new versions of existing stages (PATCH).
-- Takes path_id[] to create new versioned objects under existing paths.
-- Caller supplies content_hash for idempotent upsert.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $3::UUID[],
        $2::TEXT[],
        $1::TEXT[]
    ) WITH ORDINALITY AS o(
        path_id,
        description,
        content_hash,
        ord
    )
),
paths AS (
    SELECT
        op.id AS path_id,
        spd.name,
        spd.slug,
        args.ord
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.stage_path_details spd ON spd.path_id = op.id
),
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT paths.path_id, args.content_hash
    FROM args
    JOIN paths ON paths.ord = args.ord
    ORDER BY paths.ord
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
numbered_versions AS (
    SELECT nv.*, row_number() OVER (ORDER BY nv.id) AS ord
    FROM new_versions nv
),

new_stages AS (
    INSERT INTO rev.stages (versioned_object_id, description)
    SELECT
        nuv.id,
        args.description
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
    ORDER BY args.ord
    RETURNING id, versioned_object_id, description, created_at
),
numbered_stages AS (
    SELECT ns.*, row_number() OVER (ORDER BY ns.id) AS ord
    FROM new_stages ns
)
SELECT
    nus.id,
    nus.versioned_object_id,
    nuv.path_id,
    paths.name,
    paths.slug,
    nuv.content_hash,
    nus.description,
    nus.created_at
FROM numbered_stages nus
JOIN numbered_versions nuv ON nuv.ord = nus.ord
JOIN paths ON paths.ord = nus.ord
ORDER BY nus.id;
