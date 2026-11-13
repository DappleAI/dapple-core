WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::BIGINT[],
        $4::UUID[],
        $5::TEXT[]
    ) WITH ORDINALITY AS o(
        asset_kind_key,
        content_hash,
        content_length,
        draft_id,
        mime_type,
        ord
    )
),
kinds AS (
    SELECT
        ak.id as kind_id,
        ak.key as kind_key,
        args.mime_type,
        args.content_length,
        args.draft_id,
        args.content_hash,
        args.ord
    FROM args
    JOIN rev.asset_kinds ak ON ak.key = args.asset_kind_key
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'asset'
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
    ORDER BY kinds.ord
    RETURNING id
),
numbered_paths AS (
    SELECT np.id, row_number() OVER (ORDER BY np.id) as ord
    FROM new_paths np
),
path_details AS (
    INSERT INTO rev.asset_path_details (path_id, asset_kind_id)
    SELECT
        nup.id,
        kinds.kind_id
    FROM kinds
    JOIN numbered_paths nup ON nup.ord = kinds.ord
    RETURNING path_id, asset_kind_id
),
numbered_path_details AS (
    SELECT pd.path_id, pd.asset_kind_id, row_number() OVER (ORDER BY pd.path_id) as ord
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
    SELECT nv.*, row_number() OVER (ORDER BY nv.id) as ord
    FROM new_versions nv
),
new_assets AS (
    INSERT INTO rev.assets (versioned_object_id, draft_id, content_length, mime_type)
    SELECT
        nuv.id,
        kinds.draft_id,
        kinds.content_length,
        kinds.mime_type
    FROM kinds
    JOIN numbered_versions nuv ON nuv.ord = kinds.ord
    RETURNING id, versioned_object_id, draft_id, content_length, mime_type, created_at
),
numbered_assets AS (
    SELECT na.*, row_number() OVER (ORDER BY na.id) as ord
    FROM new_assets na
)
SELECT
    nua.id,
    nua.versioned_object_id,
    nuv.path_id,
    ak.key as asset_kind_key,
    nuv.content_hash,
    nua.draft_id,
    nua.content_length,
    nua.mime_type,
    nua.created_at
FROM numbered_assets nua
JOIN numbered_versions nuv ON nuv.ord = nua.ord
JOIN numbered_paths nup ON nup.ord = nua.ord
JOIN numbered_path_details npd ON npd.ord = nua.ord
JOIN rev.asset_kinds ak ON ak.id = npd.asset_kind_id
LEFT JOIN rev.drafts d ON d.id = nua.draft_id
ORDER BY nua.id;
