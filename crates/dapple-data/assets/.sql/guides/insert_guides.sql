WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::TEXT[],
        $4::TEXT[],
        $5::TEXT[],
        $6::TEXT[],
        $7::TEXT[]
    ) WITH ORDINALITY AS o(
        author,
        classification,
        conduits_md,
        content_hash,
        description,
        source_path,
        title,
        ord
    )
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'guide'
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
    INSERT INTO rev.guide_path_details (path_id, title, slug, namespace_company_id)
    SELECT
        np.id,
        args.title,
                                                        rev.slugify(
            COALESCE(
                NULLIF(regexp_replace(NULLIF(btrim(args.source_path), ''), '^.*/', ''), ''),
                args.title
            )
        ),
        np.namespace_company_id
    FROM args
    JOIN numbered_paths np ON np.ord = args.ord
    RETURNING path_id, title, slug, namespace_company_id
),
numbered_path_details AS (
    SELECT pd.*, row_number() OVER (ORDER BY pd.path_id) AS ord
    FROM path_details pd
),

new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT np.id, COALESCE(args.content_hash, '0000000000000000000000000000000000000000')
    FROM numbered_paths np
    JOIN args ON args.ord = np.ord
    ORDER BY np.id
    RETURNING id, path_id, content_hash, created_at
),
numbered_versions AS (
    SELECT nv.*, row_number() OVER (ORDER BY nv.id) AS ord
    FROM new_versions nv
),
new_guides AS (
    INSERT INTO rev.guides (id, versioned_object_id, description, author, classification, source_path, conduits_md)
    SELECT
        uuidv7(),
        nuv.id,
        args.description,
        args.author,
        args.classification,
        args.source_path,
                        args.conduits_md
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
    ORDER BY args.ord
    RETURNING id, versioned_object_id, description, author, classification, source_path, created_at
),
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
