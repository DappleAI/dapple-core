
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::TEXT[]
    ) WITH ORDINALITY AS o(content_hash, description, name, ord)
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'fluent_message'
),
existing_details AS (
    SELECT fmpd.path_id, op.id AS path_id_val, fmpd.name
    FROM rev.fluent_message_path_details fmpd
    JOIN rev.object_paths op ON op.id = fmpd.path_id
    WHERE fmpd.name = ANY($3::TEXT[])
      AND fmpd.namespace_company_id = COALESCE(
          NULLIF(current_setting('dapple.namespace_company_id', true), ''),
          '019e132e-2610-7a4a-b64a-77a49e286ac0'
      )::UUID
),
new_name_args AS (
    SELECT args.*
    FROM args
    LEFT JOIN existing_details ed ON ed.name = args.name
    WHERE ed.name IS NULL
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
    FROM new_name_args
    ORDER BY new_name_args.ord
        RETURNING id, namespace_company_id
),
numbered_new_paths AS (
    SELECT np.*, row_number() OVER (ORDER BY np.id) AS rn
    FROM new_paths np
),
numbered_new_name_args AS (
    SELECT nna.*, row_number() OVER (ORDER BY nna.ord) AS rn
    FROM new_name_args nna
),
new_path_details AS (
    INSERT INTO rev.fluent_message_path_details (path_id, name, description, namespace_company_id)
    SELECT nnp.id, nna.name, nna.description, nnp.namespace_company_id
    FROM numbered_new_paths nnp
    JOIN numbered_new_name_args nna ON nna.rn = nnp.rn
    RETURNING path_id, name, description, namespace_company_id
),
all_paths AS (
    SELECT ed.path_id, args.name, args.description, args.content_hash, args.ord
    FROM args
    JOIN existing_details ed ON ed.name = args.name
    UNION ALL
    SELECT nnp.id, nna.name, nna.description, nna.content_hash, nna.ord
    FROM numbered_new_paths nnp
    JOIN numbered_new_name_args nna ON nna.rn = nnp.rn
),
new_versions_inserted AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT ap.path_id, ap.content_hash
    FROM all_paths ap
    ORDER BY ap.ord
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
all_versions AS (
    SELECT nvi.id, nvi.path_id, nvi.content_hash, nvi.created_at
    FROM new_versions_inserted nvi
    UNION ALL
    SELECT vo.id, vo.path_id, vo.content_hash, vo.created_at
    FROM all_paths ap
    JOIN rev.versioned_objects vo ON vo.path_id = ap.path_id AND vo.content_hash = ap.content_hash
    WHERE NOT EXISTS (
        SELECT 1 FROM new_versions_inserted nvi
        WHERE nvi.path_id = ap.path_id AND nvi.content_hash = ap.content_hash
    )
),
new_messages AS (
    INSERT INTO rev.fluent_messages (versioned_object_id)
    SELECT av.id
    FROM all_versions av
    WHERE NOT EXISTS (SELECT 1 FROM rev.fluent_messages fm WHERE fm.versioned_object_id = av.id)
    ORDER BY av.id
    RETURNING id, versioned_object_id, created_at
),
all_messages AS (
    SELECT nm.id, nm.versioned_object_id, nm.created_at
    FROM new_messages nm
    UNION ALL
    SELECT fm.id, fm.versioned_object_id, fm.created_at
    FROM rev.fluent_messages fm
    JOIN all_versions av ON fm.versioned_object_id = av.id
    WHERE NOT EXISTS (SELECT 1 FROM new_messages nm WHERE nm.versioned_object_id = av.id)
)
SELECT
    am.id AS "id!",
    av.id AS "versioned_object_id!",
    ap.path_id AS "path_id!",
    ap.name AS "name!",
    ap.description,
    av.content_hash AS "content_hash!",
    am.created_at AS "created_at!"
FROM all_paths ap
JOIN all_versions av ON av.path_id = ap.path_id AND av.content_hash = ap.content_hash
JOIN all_messages am ON am.versioned_object_id = av.id
ORDER BY ap.ord;
