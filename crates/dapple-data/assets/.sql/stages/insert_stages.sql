WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::UUID[],
        $3::TEXT[],
                        $4::JSONB[],
        $5::TEXT[],
        $6::TEXT[]
    ) WITH ORDINALITY AS o(
        content_hash,
        description_fluent_message_id,
        description,
        icons,
        kind,
        name,
        ord
    )
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'stage'
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
    INSERT INTO rev.stage_path_details (path_id, name, slug, namespace_company_id)
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

new_stages AS (
    INSERT INTO rev.stages (versioned_object_id, description, description_fluent_message_id, kind_id, icons)
    SELECT
        nuv.id,
        args.description,
        args.description_fluent_message_id,
        sk.id,
                        NULLIF(args.icons, '[]'::JSONB)
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
                        LEFT JOIN rev.stage_kinds sk ON sk.key = args.kind
    ORDER BY args.ord
    RETURNING id, versioned_object_id, description, description_fluent_message_id, kind_id, icons, created_at
),
numbered_stages AS (
    SELECT ns.*, row_number() OVER (ORDER BY ns.id) AS ord
    FROM new_stages ns
)
SELECT
    nus.id,
    nus.versioned_object_id,
    nuv.path_id,
    npd.name,
    npd.slug,
    nuv.content_hash,
    COALESCE(rev.fluent_body(nus.description_fluent_message_id, 'en'), nus.description, npd.name) AS "description!",
    sk.key AS "kind?",
    nus.icons AS "icons?",
    nus.created_at
FROM numbered_stages nus
JOIN numbered_versions nuv ON nuv.ord = nus.ord
JOIN numbered_paths nup ON nup.ord = nus.ord
JOIN numbered_path_details npd ON npd.ord = nus.ord
LEFT JOIN rev.stage_kinds sk ON sk.id = nus.kind_id
ORDER BY nus.id;
