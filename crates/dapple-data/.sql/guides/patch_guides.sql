-- Create new versions of existing guides (PATCH).
-- Caller supplies content_hash for idempotent upsert.
-- Fluent-native: no locale column -- guide structure is locale-independent.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::TEXT[],
        $4::TEXT[],
        $5::UUID[],
        $6::TEXT[]
    ) WITH ORDINALITY AS o(
        author,
        classification,
        content_hash,
        description,
        path_id,
        source_path,
        ord
    )
),
paths AS (
    SELECT
        op.id AS path_id,
        gpd.title,
        gpd.slug,
        args.ord
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
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

new_guides AS (
    INSERT INTO rev.guides (versioned_object_id, description, author, classification, source_path)
    SELECT
        nuv.id,
        args.description,
        args.author,
        args.classification,
        args.source_path
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
    WHERE NOT EXISTS (SELECT 1 FROM rev.guides g WHERE g.versioned_object_id = nuv.id)
    ORDER BY args.ord
    RETURNING id, versioned_object_id, description, author, classification, source_path, created_at
),
all_guides AS (
    SELECT ng.id, ng.versioned_object_id, ng.description, ng.author, ng.classification, ng.source_path, ng.created_at
    FROM new_guides ng
    UNION ALL
    SELECT g.id, g.versioned_object_id, g.description, g.author, g.classification, g.source_path, g.created_at
    FROM rev.guides g
    JOIN numbered_versions nuv ON g.versioned_object_id = nuv.id
    WHERE NOT EXISTS (SELECT 1 FROM new_guides ng WHERE ng.versioned_object_id = nuv.id)
),
numbered_guides AS (
    SELECT ag.*, row_number() OVER (ORDER BY ag.id) AS ord
    FROM all_guides ag
)
SELECT
    nug.id AS "id!",
    nug.versioned_object_id AS "versioned_object_id!",
    nuv.path_id AS "path_id!",
    paths.title AS "title!",
    paths.slug AS "slug!",
    nuv.content_hash AS "content_hash!",
    nug.description,
    nug.author,
    nug.classification,
    nug.source_path,
    nug.created_at AS "created_at!"
FROM numbered_guides nug
JOIN numbered_versions nuv ON nuv.ord = nug.ord
JOIN paths ON paths.ord = nug.ord
ORDER BY nug.id;
