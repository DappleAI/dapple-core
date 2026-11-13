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
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'inference_profile'
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
    FROM args
    ORDER BY args.ord
                    RETURNING id, namespace_company_id
),
numbered_paths AS (
    SELECT np.id, np.namespace_company_id, row_number() OVER (ORDER BY np.id) AS ord
    FROM new_paths np
),

path_details AS (
    INSERT INTO rev.inference_profile_path_details (path_id, name, namespace_company_id)
    SELECT
        np.id,
        args.name,
        np.namespace_company_id
    FROM args
    JOIN numbered_paths np ON np.ord = args.ord
    RETURNING path_id, name, namespace_company_id
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

new_profiles AS (
    INSERT INTO rev.inference_profiles (versioned_object_id, name, description)
    SELECT
        nuv.id,
        args.name,
        args.description
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
    ORDER BY args.ord
    RETURNING id, versioned_object_id, name, description, created_at
),
numbered_profiles AS (
    SELECT np.*, row_number() OVER (ORDER BY np.id) AS ord
    FROM new_profiles np
)
SELECT
    nup.id AS "id!",
    nup.versioned_object_id AS "versioned_object_id!",
    nuv.path_id AS "path_id!",
    npd.name AS "name!",
    nuv.content_hash AS "content_hash!",
    nup.description,
    nup.created_at AS "created_at!"
FROM numbered_profiles nup
JOIN numbered_versions nuv ON nuv.ord = nup.ord
JOIN numbered_paths nupa ON nupa.ord = nup.ord
JOIN numbered_path_details npd ON npd.ord = nup.ord
ORDER BY nup.id;
