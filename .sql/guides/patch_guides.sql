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
    ORDER BY args.ord
    RETURNING id, versioned_object_id, description, author, classification, source_path, created_at
),
numbered_guides AS (
    SELECT ng.*, row_number() OVER (ORDER BY ng.id) AS ord
    FROM new_guides ng
)
SELECT
    nug.id,
    nug.versioned_object_id,
    nuv.path_id,
    paths.title,
    paths.slug,
    nuv.content_hash,
    nug.description,
    nug.author,
    nug.classification,
    nug.source_path,
    nug.created_at
FROM numbered_guides nug
JOIN numbered_versions nuv ON nuv.ord = nug.ord
JOIN paths ON paths.ord = nug.ord
ORDER BY nug.id;
