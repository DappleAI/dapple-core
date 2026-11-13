WITH args AS (
    SELECT *
    FROM UNNEST(
        $3::TEXT[],
        $2::TEXT[],
        $1::TEXT[]
    ) WITH ORDINALITY AS o(
        name,
        description,
        content_hash,
        ord
    )
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'playbook'
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
    INSERT INTO rev.playbook_path_details (path_id, name, slug, namespace_company_id)
    SELECT
        np.id,
        args.name,
        rev.slugify(args.name),
        np.namespace_company_id
    FROM args
    JOIN numbered_paths np ON np.ord = args.ord
    RETURNING path_id, name, slug, namespace_company_id
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

new_playbooks AS (
    INSERT INTO rev.playbooks (versioned_object_id, description)
    SELECT
        nuv.id,
        args.description
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
    ORDER BY args.ord
    RETURNING id, versioned_object_id, description, created_at
),
numbered_playbooks AS (
    SELECT np.*, row_number() OVER (ORDER BY np.id) AS ord
    FROM new_playbooks np
)
SELECT
    nup.id,
    nup.versioned_object_id,
    nuv.path_id,
    npd.name,
    npd.slug,
    nuv.content_hash,
    nup.description,
    nup.created_at
FROM numbered_playbooks nup
JOIN numbered_versions nuv ON nuv.ord = nup.ord
JOIN numbered_paths nupp ON nupp.ord = nup.ord
JOIN numbered_path_details npd ON npd.ord = nup.ord
ORDER BY nup.id;
