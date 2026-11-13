WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::UUID[],
        $3::TEXT[],
        $4::TEXT[],
        $5::JSONB[],
        $6::UUID[],
        $7::TEXT[]
    ) WITH ORDINALITY AS o(
        content_hash,
        description_fluent_message_id,
        description,
        name,
        pack_spec,
        title_fluent_message_id,
        title,
        ord
    )
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'brief_pack'
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
    INSERT INTO rev.brief_pack_path_details (path_id, name, namespace_company_id)
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

new_packs AS (
    INSERT INTO rev.brief_packs (
        versioned_object_id,
        title,
        description,
        pack_spec,
        title_fluent_message_id,
        description_fluent_message_id
    )
    SELECT
        nuv.id,
        args.title,
        args.description,
        COALESCE(args.pack_spec, '{}'::jsonb),
        args.title_fluent_message_id,
        args.description_fluent_message_id
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
    ORDER BY args.ord
    RETURNING
        id,
        versioned_object_id,
        title,
        description,
        pack_spec,
        title_fluent_message_id,
        description_fluent_message_id,
        created_at
),
numbered_packs AS (
    SELECT np.*, row_number() OVER (ORDER BY np.id) AS ord
    FROM new_packs np
)
SELECT
    nup.id,
    nup.versioned_object_id,
    nupp.id AS path_id,
    npd.name,
    nuv.content_hash,
    COALESCE(rev.fluent_body(nup.title_fluent_message_id, 'en'), nup.title, npd.name) AS "title!",
    COALESCE(rev.fluent_body(nup.description_fluent_message_id, 'en'), nup.description) AS description,
    nup.pack_spec,
    nup.created_at
FROM numbered_packs nup
JOIN numbered_versions nuv ON nuv.ord = nup.ord
JOIN numbered_paths nupp ON nupp.ord = nup.ord
JOIN numbered_path_details npd ON npd.ord = nup.ord
ORDER BY nup.id;
