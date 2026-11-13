-- Create new versions of existing playbooks (PATCH).
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
        ppd.name,
        ppd.slug,
        args.ord
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.playbook_path_details ppd ON ppd.path_id = op.id
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

new_playbooks AS (
    INSERT INTO rev.playbooks (versioned_object_id, description)
    SELECT
        nuv.id,
        args.description
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
    WHERE NOT EXISTS (SELECT 1 FROM rev.playbooks p WHERE p.versioned_object_id = nuv.id)
    ORDER BY args.ord
    RETURNING id, versioned_object_id, description, created_at
),
all_playbooks AS (
    SELECT np.id, np.versioned_object_id, np.description, np.created_at
    FROM new_playbooks np
    UNION ALL
    SELECT p.id, p.versioned_object_id, p.description, p.created_at
    FROM rev.playbooks p
    JOIN numbered_versions nuv ON p.versioned_object_id = nuv.id
    WHERE NOT EXISTS (SELECT 1 FROM new_playbooks np WHERE np.versioned_object_id = nuv.id)
),
numbered_playbooks AS (
    SELECT ap.*, row_number() OVER (ORDER BY ap.id) AS ord
    FROM all_playbooks ap
)
SELECT
    nup.id AS "id!",
    nup.versioned_object_id AS "versioned_object_id!",
    nuv.path_id AS "path_id!",
    paths.name AS "name!",
    paths.slug AS "slug!",
    nuv.content_hash AS "content_hash!",
    nup.description AS "description!",
    nup.created_at AS "created_at!"
FROM numbered_playbooks nup
JOIN numbered_versions nuv ON nuv.ord = nup.ord
JOIN paths ON paths.ord = nup.ord
ORDER BY nup.id;
