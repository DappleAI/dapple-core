WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::UUID[],
        $3::TEXT[],
        $4::UUID[],
        $5::JSONB[],
        $6::UUID[],
        $7::TEXT[]
    ) WITH ORDINALITY AS o(
        content_hash,
        description_fluent_message_id,
        description,
        id,
        pack_spec,
        title_fluent_message_id,
        title,
        ord
    )
),
existing_packs AS (
    SELECT
        vo.path_id,
        args.id as old_id,
        args.title,
        args.description,
        args.title_fluent_message_id,
        args.description_fluent_message_id,
        args.pack_spec,
        args.content_hash,
        args.ord
    FROM args
    JOIN rev.brief_packs bp ON bp.id = args.id
    JOIN rev.versioned_objects vo ON vo.id = bp.versioned_object_id
),
new_versions_inserted AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        ep.path_id,
        ep.content_hash
    FROM existing_packs ep
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
all_versions AS (
    SELECT nvi.id, nvi.path_id, nvi.content_hash, nvi.created_at
    FROM new_versions_inserted nvi
    UNION ALL
    SELECT vo.id, vo.path_id, vo.content_hash, vo.created_at
    FROM existing_packs ep
    JOIN rev.versioned_objects vo ON vo.path_id = ep.path_id AND vo.content_hash = ep.content_hash
    WHERE NOT EXISTS (
        SELECT 1 FROM new_versions_inserted nvi
        WHERE nvi.path_id = ep.path_id AND nvi.content_hash = ep.content_hash
    )
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
        av.id,
        ep.title,
        ep.description,
        COALESCE(ep.pack_spec, '{}'::jsonb),
        ep.title_fluent_message_id,
        ep.description_fluent_message_id
    FROM existing_packs ep
    JOIN all_versions av ON av.path_id = ep.path_id
    WHERE NOT EXISTS (SELECT 1 FROM rev.brief_packs bp WHERE bp.versioned_object_id = av.id)
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
all_packs AS (
    SELECT
        np.id,
        np.versioned_object_id,
        np.title,
        np.description,
        np.title_fluent_message_id,
        np.description_fluent_message_id,
        np.pack_spec,
        np.created_at
    FROM new_packs np
    UNION ALL
    SELECT
        bp.id,
        bp.versioned_object_id,
        bp.title,
        bp.description,
        bp.title_fluent_message_id,
        bp.description_fluent_message_id,
        bp.pack_spec,
        bp.created_at
    FROM rev.brief_packs bp
    JOIN all_versions av ON bp.versioned_object_id = av.id
    WHERE NOT EXISTS (SELECT 1 FROM new_packs np WHERE np.versioned_object_id = av.id)
)
SELECT
    ap.id AS "id!",
    ap.versioned_object_id AS "versioned_object_id!",
    op.id AS "path_id!",
    bppd.name AS "name!",
    av.content_hash AS "content_hash!",
    COALESCE(rev.fluent_body(ap.title_fluent_message_id, 'en'), ap.title, bppd.name) AS "title!",
    COALESCE(rev.fluent_body(ap.description_fluent_message_id, 'en'), ap.description) AS description,
    ap.pack_spec AS "pack_spec!",
    ap.created_at AS "created_at!"
FROM all_packs ap
JOIN all_versions av ON av.id = ap.versioned_object_id
JOIN rev.object_paths op ON op.id = av.path_id
JOIN rev.brief_pack_path_details bppd ON bppd.path_id = op.id
JOIN existing_packs ep ON ep.path_id = av.path_id
ORDER BY ap.id;
