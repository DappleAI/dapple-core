-- Insert value slots using the unified versioning infrastructure
-- Creates: object_path → value_slot_path_details → versioned_object → value_slot
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::JSONB[],
        $2::TEXT[],
        $3::TEXT[],
        $4::TEXT[],
        $5::TEXT[],
        $6::TEXT[]
    ) WITH ORDINALITY AS o(
        constraints,
        content_hash,
        description,
        name,
        title,
        value_kind_key,
        ord
    )
),
-- Look up value kind IDs
kinds AS (
    SELECT
        vk.id as kind_id,
        vk.key as kind_key,
        args.name,
        args.title,
        args.description,
        args.constraints,
        args.content_hash,
        args.ord
    FROM args
    JOIN rev.value_kinds vk ON vk.key = args.value_kind_key
),
-- Get the entity type ID for value_slots
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'value_slot'
),
-- Create object paths
new_paths AS (
    INSERT INTO rev.object_paths (entity_type_id)
    SELECT (SELECT id FROM entity_type)
    FROM kinds
    RETURNING id
),
-- Create path details (name + value_kind_id)
path_details AS (
    INSERT INTO rev.value_slot_path_details (path_id, name, value_kind_id)
    SELECT
        np.id,
        kinds.name,
        kinds.kind_id
    FROM kinds
    JOIN new_paths np ON np.id = (
        SELECT id FROM new_paths
        ORDER BY id
        LIMIT 1 OFFSET kinds.ord - 1
    )
    RETURNING path_id, name, value_kind_id
),
-- Number the paths by id to correlate with kinds.ord
numbered_paths AS (
    SELECT np.*, row_number() OVER (ORDER BY np.id) AS ord
    FROM new_paths np
),
numbered_path_details AS (
    SELECT pd.*, row_number() OVER (ORDER BY pd.path_id) AS ord
    FROM path_details pd
),

-- Create versioned objects with content hash
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT nup.id, kinds.content_hash
    FROM numbered_paths nup
    JOIN kinds ON kinds.ord = nup.ord
    ORDER BY nup.ord
    RETURNING id, path_id, content_hash, created_at
),
-- Number versions to preserve ordinality
numbered_versions AS (
    SELECT nv.*, row_number() OVER (ORDER BY nv.id) AS ord
    FROM new_versions nv
),
-- Create value slots
new_slots AS (
    INSERT INTO rev.value_slots (versioned_object_id, title, description, constraints)
    SELECT
        nuv.id,
        kinds.title,
        kinds.description,
        COALESCE(kinds.constraints, '{}'::jsonb)
    FROM kinds
    JOIN numbered_versions nuv ON nuv.ord = kinds.ord
    ORDER BY kinds.ord
    RETURNING id, versioned_object_id, title, description, constraints, created_at
),
-- Number slots to preserve ordinality
numbered_slots AS (
    SELECT ns.*, row_number() OVER (ORDER BY ns.id) AS ord
    FROM new_slots ns
)
SELECT
    nus.id,
    nus.versioned_object_id,
    nup.id AS path_id,
    npd.name,
    vk.key as value_kind_key,
    nuv.content_hash,
    nus.title,
    nus.description,
    nus.constraints,
    nus.created_at
FROM numbered_slots nus
JOIN numbered_versions nuv ON nuv.ord = nus.ord
JOIN numbered_paths nup ON nup.ord = nus.ord
JOIN numbered_path_details npd ON npd.ord = nus.ord
JOIN rev.value_kinds vk ON vk.id = npd.value_kind_id
ORDER BY nus.id;
