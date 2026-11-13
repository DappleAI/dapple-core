WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::BIGINT[],
        $3::UUID[],
        $4::UUID[],
        $5::TEXT[]
    ) WITH ORDINALITY AS o(content_hash, content_length, draft_id, id, mime_type, ord)
),
existing_assets AS (
    SELECT
        vo.path_id,
        args.id as old_id,
        args.mime_type,
        args.content_length,
        args.draft_id,
        args.content_hash,
        args.ord
    FROM args
    JOIN rev.assets a ON a.id = args.id
    JOIN rev.versioned_objects vo ON vo.id = a.versioned_object_id
),
new_versions_inserted AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        ea.path_id,
        ea.content_hash
    FROM existing_assets ea
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
all_versions AS (
    SELECT nvi.id, nvi.path_id, nvi.content_hash, nvi.created_at
    FROM new_versions_inserted nvi
    UNION ALL
    SELECT vo.id, vo.path_id, vo.content_hash, vo.created_at
    FROM existing_assets ea
    JOIN rev.versioned_objects vo ON vo.path_id = ea.path_id AND vo.content_hash = ea.content_hash
    WHERE NOT EXISTS (
        SELECT 1 FROM new_versions_inserted nvi
        WHERE nvi.path_id = ea.path_id AND nvi.content_hash = ea.content_hash
    )
),
new_assets AS (
    INSERT INTO rev.assets (versioned_object_id, draft_id, content_length, mime_type)
    SELECT
        av.id,
        ea.draft_id,
        ea.content_length,
        ea.mime_type
    FROM existing_assets ea
    JOIN all_versions av ON av.path_id = ea.path_id
    WHERE NOT EXISTS (SELECT 1 FROM rev.assets a WHERE a.versioned_object_id = av.id)
    RETURNING id, versioned_object_id, draft_id, content_length, mime_type, created_at
),
all_assets AS (
    SELECT na.id, na.versioned_object_id, na.draft_id, na.content_length, na.mime_type, na.created_at
    FROM new_assets na
    UNION ALL
    SELECT a.id, a.versioned_object_id, a.draft_id, a.content_length, a.mime_type, a.created_at
    FROM rev.assets a
    JOIN all_versions av ON a.versioned_object_id = av.id
    WHERE NOT EXISTS (SELECT 1 FROM new_assets na WHERE na.versioned_object_id = av.id)
)
SELECT
    aa.id AS "id!",
    aa.versioned_object_id AS "versioned_object_id!",
    av.path_id AS "path_id!",
    ak.key AS "asset_kind_key!",
    av.content_hash AS "content_hash!",
    aa.draft_id,
    aa.content_length AS "content_length!",
    aa.mime_type AS "mime_type!",
    aa.created_at AS "created_at!"
FROM all_assets aa
JOIN all_versions av ON av.id = aa.versioned_object_id
JOIN rev.object_paths op ON op.id = av.path_id
JOIN rev.asset_path_details apd ON apd.path_id = op.id
JOIN rev.asset_kinds ak ON ak.id = apd.asset_kind_id
JOIN existing_assets ea ON ea.path_id = av.path_id
LEFT JOIN rev.drafts d ON d.id = aa.draft_id
ORDER BY aa.id;
