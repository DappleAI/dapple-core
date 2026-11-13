-- Insert hats using the unified versioning infrastructure
-- Creates: object_path → hat_path_details → versioned_object → hat
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::TEXT[]
    ) WITH ORDINALITY AS o(
        content_hash,
        description,
        name,
        ord
    )
),
-- Get the entity type ID for hats
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'hat'
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

-- Create path details (name, slug)
path_details AS (
    INSERT INTO rev.hat_path_details (path_id, name, slug)
    SELECT
        np.id,
        args.name,
        rev.slugify(args.name)
    FROM args
    JOIN numbered_paths np ON np.ord = args.ord
    RETURNING path_id, name, slug
),
numbered_path_details AS (
    SELECT pd.*, row_number() OVER (ORDER BY pd.path_id) AS ord
    FROM path_details pd
),

new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT np.id, args.content_hash
    FROM numbered_paths np
    JOIN args ON args.ord = np.ord
    ORDER BY np.id
    RETURNING id, path_id, content_hash, created_at
),
numbered_versions AS (
    SELECT nv.*, row_number() OVER (ORDER BY nv.id) AS ord
    FROM new_versions nv
),

-- Create hats
new_hats AS (
    INSERT INTO rev.hats (versioned_object_id, description)
    SELECT
        nuv.id,
        args.description
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
    ORDER BY args.ord
    RETURNING id, versioned_object_id, description, created_at
),
-- Number hats to preserve ordinality
numbered_hats AS (
    SELECT nh.*, row_number() OVER (ORDER BY nh.id) AS ord
    FROM new_hats nh
)
SELECT
    nuh.id,
    nuh.versioned_object_id,
    nuv.path_id,
    npd.name,
    npd.slug,
    nuv.content_hash,
    nuh.description,
    nuh.created_at
FROM numbered_hats nuh
JOIN numbered_versions nuv ON nuv.ord = nuh.ord
JOIN numbered_paths nup ON nup.ord = nuh.ord
JOIN numbered_path_details npd ON npd.ord = nuh.ord
ORDER BY nuh.id;
