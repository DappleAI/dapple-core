WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::JSONB[],
        $3::TEXT[],
        $4::TEXT[],
        $5::INT[],
        $6::INT[],
        $7::TEXT[],
        $8::TEXT[],
        $9::TEXT[]
    ) WITH ORDINALITY AS o(
        cardinality,
        constraints,
        content_hash,
        description,
        max_selections,
        min_selections,
        name,
        title,
        value_kind_key,
        ord
    )
),
kinds AS (
    SELECT
        vk.id as kind_id,
        vk.key as kind_key,
        args.name,
        args.title,
        args.description,
        args.constraints,
        args.cardinality,
        args.min_selections,
        args.max_selections,
        args.content_hash,
        args.ord
    FROM args
    JOIN rev.value_kinds vk ON vk.key = args.value_kind_key
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'value_slot'
),
new_paths AS (
    INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope)
    SELECT
        (SELECT id FROM entity_type),
        COALESCE(
            NULLIF(current_setting('dapple.namespace_company_id', true), ''),
            '019e132e-2610-7a4a-b64a-77a49e286ac0'
        )::UUID,
        COALESCE(
            NULLIF(current_setting('dapple.scope', true), ''),
            'public'
        )::TEXT
    FROM kinds
        RETURNING id, namespace_company_id
),
path_details AS (
    INSERT INTO rev.value_slot_path_details (path_id, name, value_kind_id, namespace_company_id)
    SELECT
        np.id,
        kinds.name,
        kinds.kind_id,
        np.namespace_company_id
    FROM kinds
    JOIN new_paths np ON np.id = (
        SELECT id FROM new_paths
        ORDER BY id
        LIMIT 1 OFFSET kinds.ord - 1
    )
    RETURNING path_id, name, value_kind_id, namespace_company_id
),
numbered_paths AS (
    SELECT np.*, row_number() OVER (ORDER BY np.id) AS ord
    FROM new_paths np
),
numbered_path_details AS (
    SELECT pd.*, row_number() OVER (ORDER BY pd.path_id) AS ord
    FROM path_details pd
),

new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT nup.id, kinds.content_hash
    FROM numbered_paths nup
    JOIN kinds ON kinds.ord = nup.ord
    ORDER BY nup.ord
    RETURNING id, path_id, content_hash, created_at
),
numbered_versions AS (
    SELECT nv.*, row_number() OVER (ORDER BY nv.id) AS ord
    FROM new_versions nv
),
new_slots AS (
    INSERT INTO rev.value_slots (
        versioned_object_id, title, description, constraints,
        cardinality, min_selections, max_selections, value_kind_id
    )
    SELECT
        nuv.id,
        kinds.title,
        kinds.description,
        COALESCE(kinds.constraints, '{}'::jsonb),
        COALESCE(kinds.cardinality, 'single'),
        kinds.min_selections,
        kinds.max_selections,
        kinds.kind_id
    FROM kinds
    JOIN numbered_versions nuv ON nuv.ord = kinds.ord
    ORDER BY kinds.ord
    RETURNING
        id, versioned_object_id, title, description, constraints,
        cardinality, min_selections, max_selections, value_kind_id, created_at
),
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
    nus.cardinality,
    nus.min_selections,
    nus.max_selections,
    nus.created_at
FROM numbered_slots nus
JOIN numbered_versions nuv ON nuv.ord = nus.ord
JOIN numbered_paths nup ON nup.ord = nus.ord
JOIN numbered_path_details npd ON npd.ord = nus.ord
JOIN rev.value_kinds vk ON vk.id = nus.value_kind_id
ORDER BY nus.id;
