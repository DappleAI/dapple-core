-- Insert guides using the unified versioning infrastructure
-- Creates: object_path -> guide_path_details -> versioned_object -> guide
-- Fluent-native: no locale column -- guide structure is locale-independent.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::TEXT[],
        $4::TEXT[],
        $5::TEXT[],
        $6::TEXT[]
    ) WITH ORDINALITY AS o(
        author,
        classification,
        content_hash,
        description,
        source_path,
        title,
        ord
    )
),
-- Get the entity type ID for guides
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'guide'
),
new_paths AS (
    INSERT INTO rev.object_paths (entity_type_id)
    SELECT (SELECT id FROM entity_type)
    FROM args
    ORDER BY args.ord
    RETURNING id
),
numbered_paths AS (
    SELECT np.*, row_number() OVER (ORDER BY np.id) AS ord
    FROM new_paths np
),

-- Create path details (title, slug)
path_details AS (
    INSERT INTO rev.guide_path_details (path_id, title, slug)
    SELECT
        np.id,
        args.title,
        rev.slugify(args.title)
    FROM args
    JOIN numbered_paths np ON np.ord = args.ord
    RETURNING path_id, title, slug
),
numbered_path_details AS (
    SELECT pd.*, row_number() OVER (ORDER BY pd.path_id) AS ord
    FROM path_details pd
),

-- Create versioned objects with content hash (COALESCE for guides)
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT np.id, COALESCE(args.content_hash, '0000000000000000000000000000000000000000')
    FROM numbered_paths np
    JOIN args ON args.ord = np.ord
    ORDER BY np.id
    RETURNING id, path_id, content_hash, created_at
),
-- Number versions to preserve ordinality
numbered_versions AS (
    SELECT nv.*, row_number() OVER (ORDER BY nv.id) AS ord
    FROM new_versions nv
),
-- Create guides
new_guides AS (
    INSERT INTO rev.guides (id, versioned_object_id, description, author, classification, source_path)
    SELECT
        uuidv7(),
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
-- Number guides to preserve ordinality
numbered_guides AS (
    SELECT ng.*, row_number() OVER (ORDER BY ng.id) AS ord
    FROM new_guides ng
)
SELECT
    nug.id,
    nug.versioned_object_id,
    nuv.path_id,
    npd.title,
    npd.slug,
    nuv.content_hash,
    nug.description,
    nug.author,
    nug.classification,
    nug.source_path,
    nug.created_at
FROM numbered_guides nug
JOIN numbered_versions nuv ON nuv.ord = nug.ord
JOIN numbered_paths nup ON nup.ord = nug.ord
JOIN numbered_path_details npd ON npd.ord = nug.ord
ORDER BY nug.id;
