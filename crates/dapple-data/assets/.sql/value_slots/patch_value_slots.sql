WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::JSONB[],
        $3::TEXT[],
        $4::TEXT[],
        $5::UUID[],
        $6::INT[],
        $7::INT[],
        $8::TEXT[],
        $9::TEXT[]
    ) WITH ORDINALITY AS o(
        cardinality,
        constraints,
        content_hash,
        description,
        id,
        max_selections,
        min_selections,
        title,
        value_kind_key,
        ord
    )
),
existing_slots AS (
    SELECT
        vo.path_id,
        args.id as old_id,
        args.title,
        args.description,
        args.constraints,
        args.cardinality,
        args.min_selections,
        args.max_selections,
        args.content_hash,
        vk_in.id AS value_kind_id,
        args.ord
    FROM args
    JOIN rev.value_slots vs ON vs.id = args.id
    JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
    JOIN rev.value_kinds vk_in ON vk_in.key = args.value_kind_key
),
new_versions_inserted AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        es.path_id,
        es.content_hash
    FROM existing_slots es
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
all_versions AS (
    SELECT nvi.id, nvi.path_id, nvi.content_hash, nvi.created_at
    FROM new_versions_inserted nvi
    UNION ALL
    SELECT vo.id, vo.path_id, vo.content_hash, vo.created_at
    FROM existing_slots es
    JOIN rev.versioned_objects vo ON vo.path_id = es.path_id AND vo.content_hash = es.content_hash
    WHERE NOT EXISTS (
        SELECT 1 FROM new_versions_inserted nvi
        WHERE nvi.path_id = es.path_id AND nvi.content_hash = es.content_hash
    )
),
new_slots AS (
    INSERT INTO rev.value_slots (
        versioned_object_id, title, description, constraints,
        cardinality, min_selections, max_selections, value_kind_id
    )
    SELECT
        av.id,
        es.title,
        es.description,
        COALESCE(es.constraints, '{}'::jsonb),
        COALESCE(es.cardinality, 'single'),
        es.min_selections,
        es.max_selections,
        es.value_kind_id
    FROM existing_slots es
    JOIN all_versions av ON av.path_id = es.path_id
    WHERE NOT EXISTS (SELECT 1 FROM rev.value_slots vs WHERE vs.versioned_object_id = av.id)
    RETURNING
        id, versioned_object_id, title, description, constraints,
        cardinality, min_selections, max_selections, value_kind_id, created_at
),
all_slots AS (
    SELECT
        ns.id, ns.versioned_object_id, ns.title, ns.description, ns.constraints,
        ns.cardinality, ns.min_selections, ns.max_selections, ns.value_kind_id, ns.created_at
    FROM new_slots ns
    UNION ALL
    SELECT
        vs.id, vs.versioned_object_id, vs.title, vs.description, vs.constraints,
        vs.cardinality, vs.min_selections, vs.max_selections, vs.value_kind_id, vs.created_at
    FROM rev.value_slots vs
    JOIN all_versions av ON vs.versioned_object_id = av.id
    WHERE NOT EXISTS (SELECT 1 FROM new_slots ns WHERE ns.versioned_object_id = av.id)
)
SELECT
    als.id AS "id!",
    als.versioned_object_id AS "versioned_object_id!",
    op.id AS "path_id!",
    vspd.name AS "name!",
    vk.key AS "value_kind_key!",
    av.content_hash AS "content_hash!",
    als.title AS "title!",
    als.description,
    als.constraints AS "constraints!",
    als.cardinality AS "cardinality!",
    als.min_selections,
    als.max_selections,
    als.created_at AS "created_at!"
FROM all_slots als
JOIN all_versions av ON av.id = als.versioned_object_id
JOIN rev.object_paths op ON op.id = av.path_id
JOIN rev.value_slot_path_details vspd ON vspd.path_id = op.id
JOIN rev.value_kinds vk ON vk.id = als.value_kind_id
JOIN existing_slots es ON es.path_id = av.path_id
ORDER BY als.id;
