WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],          $2::TEXT[]       ) WITH ORDINALITY AS o(name, description, ord)
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'fluent_message'
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
        RETURNING id, namespace_company_id, created_at
),
numbered_paths AS (
    SELECT np.*, row_number() OVER (ORDER BY np.id) AS ord
    FROM new_paths np
),
path_details AS (
    INSERT INTO rev.fluent_message_path_details (path_id, name, description, namespace_company_id)
    SELECT
        np.id,
        args.name,
        args.description,
        np.namespace_company_id
    FROM args
    JOIN numbered_paths np ON np.ord = args.ord
    RETURNING path_id, name, description, namespace_company_id
),
numbered_path_details AS (
    SELECT pd.*, row_number() OVER (ORDER BY pd.path_id) AS ord
    FROM path_details pd
)
SELECT
    nup.id,
    npd.name,
    npd.description,
    nup.created_at
FROM numbered_paths nup
JOIN numbered_path_details npd ON npd.ord = nup.ord
ORDER BY nup.id;
