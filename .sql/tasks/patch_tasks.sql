-- Create new versions of existing tasks (PATCH).
-- Takes path_id[] to create new versioned objects under existing paths.
-- Caller supplies content_hash for idempotent upsert.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $5::UUID[],
        $2::TEXT[],
        $4::TEXT[],
        $3::JSONB[],
        $1::TEXT[]
    ) WITH ORDINALITY AS o(
        path_id,
        description,
        function,
        function_input,
        content_hash,
        ord
    )
),
paths AS (
    SELECT
        op.id AS path_id,
        tpd.name,
        tpd.slug,
        args.ord
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.task_path_details tpd ON tpd.path_id = op.id
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

new_tasks AS (
    INSERT INTO rev.tasks (versioned_object_id, description, function, function_input)
    SELECT
        nuv.id,
        args.description,
        args.function,
        args.function_input
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
    ORDER BY args.ord
    RETURNING id, versioned_object_id, description, function, function_input, created_at
),
numbered_tasks AS (
    SELECT nt.*, row_number() OVER (ORDER BY nt.id) AS ord
    FROM new_tasks nt
)
SELECT
    nut.id,
    nut.versioned_object_id,
    nuv.path_id,
    paths.name,
    paths.slug,
    nuv.content_hash,
    nut.description,
    nut.function,
    nut.function_input,
    nut.created_at
FROM numbered_tasks nut
JOIN numbered_versions nuv ON nuv.ord = nut.ord
JOIN paths ON paths.ord = nut.ord
ORDER BY nut.id;
