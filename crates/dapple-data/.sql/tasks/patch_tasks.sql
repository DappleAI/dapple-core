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
-- Try to insert new versioned_objects. If the (path_id, content_hash) already
-- exists (content reverted to a prior version), fall back to the existing row.
new_versions_inserted AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT paths.path_id, args.content_hash
    FROM args
    JOIN paths ON paths.ord = args.ord
    ORDER BY paths.ord
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
new_versions AS (
    -- Newly inserted rows
    SELECT nvi.id, nvi.path_id, nvi.content_hash, nvi.created_at
    FROM new_versions_inserted nvi
    UNION ALL
    -- Already-existing rows (conflict hit — content reverted to prior version)
    SELECT vo.id, vo.path_id, vo.content_hash, vo.created_at
    FROM args
    JOIN paths ON paths.ord = args.ord
    JOIN rev.versioned_objects vo ON vo.path_id = paths.path_id AND vo.content_hash = args.content_hash
    WHERE NOT EXISTS (
        SELECT 1 FROM new_versions_inserted nvi
        WHERE nvi.path_id = paths.path_id AND nvi.content_hash = args.content_hash
    )
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
    WHERE NOT EXISTS (SELECT 1 FROM rev.tasks t WHERE t.versioned_object_id = nuv.id)
    ORDER BY args.ord
    RETURNING id, versioned_object_id, description, function, function_input, created_at
),
all_tasks AS (
    SELECT nt.id, nt.versioned_object_id, nt.description, nt.function, nt.function_input, nt.created_at
    FROM new_tasks nt
    UNION ALL
    SELECT t.id, t.versioned_object_id, t.description, t.function, t.function_input, t.created_at
    FROM rev.tasks t
    JOIN numbered_versions nuv ON t.versioned_object_id = nuv.id
    WHERE NOT EXISTS (SELECT 1 FROM new_tasks nt WHERE nt.versioned_object_id = nuv.id)
),
numbered_tasks AS (
    SELECT at.*, row_number() OVER (ORDER BY at.id) AS ord
    FROM all_tasks at
)
SELECT
    nut.id AS "id!",
    nut.versioned_object_id AS "versioned_object_id!",
    nuv.path_id AS "path_id!",
    paths.name AS "name!",
    paths.slug AS "slug!",
    nuv.content_hash AS "content_hash!",
    nut.description AS "description!",
    nut.function AS "function!",
    nut.function_input AS "function_input!",
    nut.created_at AS "created_at!"
FROM numbered_tasks nut
JOIN numbered_versions nuv ON nuv.ord = nut.ord
JOIN paths ON paths.ord = nut.ord
ORDER BY nut.id;
