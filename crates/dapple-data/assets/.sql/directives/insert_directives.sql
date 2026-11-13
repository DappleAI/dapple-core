WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::JSONB[],
        $4::INT[],
        $5::TEXT[],
        $6::INT[]
    ) WITH ORDINALITY AS o(
        content_hash,
        description,
        logic,
        max_attempts,
        name,
        retry_delay_ms,
        ord
    )
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'directive'
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
    INSERT INTO rev.directive_path_details (path_id, name, namespace_company_id)
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

new_directives AS (
    INSERT INTO rev.directives (versioned_object_id, logic, max_attempts, retry_delay_ms, description)
    SELECT
        nuv.id,
        args.logic,
        COALESCE(args.max_attempts, 1),
        args.retry_delay_ms,
        args.description
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
    ORDER BY args.ord
    RETURNING id, versioned_object_id, logic, max_attempts, retry_delay_ms, description, created_at
),
numbered_directives AS (
    SELECT nd.*, row_number() OVER (ORDER BY nd.id) AS ord
    FROM new_directives nd
)
SELECT
    nud.id,
    nud.versioned_object_id,
    nup.id AS path_id,
    npd.name,
    nuv.content_hash,
    nud.logic,
    nud.max_attempts,
    nud.retry_delay_ms,
    nud.description,
    nud.created_at
FROM numbered_directives nud
JOIN numbered_versions nuv ON nuv.ord = nud.ord
JOIN numbered_paths nup ON nup.ord = nud.ord
JOIN numbered_path_details npd ON npd.ord = nud.ord
ORDER BY nud.id;
