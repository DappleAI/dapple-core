WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::UUID[]
    ) WITH ORDINALITY AS o(content_hash, description, id, ord)
),
existing_profiles AS (
    SELECT
        vo.path_id,
        args.id as old_id,
        args.description,
        args.content_hash,
        args.ord
    FROM args
    JOIN rev.inference_profiles ip ON ip.id = args.id
    JOIN rev.versioned_objects vo ON vo.id = ip.versioned_object_id
),
new_versions_inserted AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        ep.path_id,
        ep.content_hash
    FROM existing_profiles ep
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
all_versions AS (
    SELECT nvi.id, nvi.path_id, nvi.content_hash, nvi.created_at
    FROM new_versions_inserted nvi
    UNION ALL
    SELECT vo.id, vo.path_id, vo.content_hash, vo.created_at
    FROM existing_profiles ep
    JOIN rev.versioned_objects vo ON vo.path_id = ep.path_id AND vo.content_hash = ep.content_hash
    WHERE NOT EXISTS (
        SELECT 1 FROM new_versions_inserted nvi
        WHERE nvi.path_id = ep.path_id AND nvi.content_hash = ep.content_hash
    )
),
new_profiles AS (
    INSERT INTO rev.inference_profiles (versioned_object_id, name, description)
    SELECT
        av.id,
        ippd.name,
        ep.description
    FROM existing_profiles ep
    JOIN all_versions av ON av.path_id = ep.path_id
    JOIN rev.inference_profile_path_details ippd ON ippd.path_id = ep.path_id
    WHERE NOT EXISTS (SELECT 1 FROM rev.inference_profiles ip WHERE ip.versioned_object_id = av.id)
    RETURNING id, versioned_object_id, name, description, created_at
),
all_profiles AS (
    SELECT np.id, np.versioned_object_id, np.name, np.description, np.created_at
    FROM new_profiles np
    UNION ALL
    SELECT ip.id, ip.versioned_object_id, ip.name, ip.description, ip.created_at
    FROM rev.inference_profiles ip
    JOIN all_versions av ON ip.versioned_object_id = av.id
    WHERE NOT EXISTS (SELECT 1 FROM new_profiles np WHERE np.versioned_object_id = av.id)
)
SELECT
    ap.id AS "id!",
    ap.versioned_object_id AS "versioned_object_id!",
    op.id AS "path_id!",
    ippd.name AS "name!",
    av.content_hash AS "content_hash!",
    ap.description,
    ap.created_at AS "created_at!"
FROM all_profiles ap
JOIN all_versions av ON av.id = ap.versioned_object_id
JOIN rev.object_paths op ON op.id = av.path_id
JOIN rev.inference_profile_path_details ippd ON ippd.path_id = op.id
ORDER BY ap.id;
