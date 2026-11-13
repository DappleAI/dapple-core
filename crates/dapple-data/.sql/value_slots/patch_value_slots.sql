-- Patch value slots using unified versioning (create new version with caller-supplied content_hash)
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::JSONB[],
        $2::TEXT[],
        $3::TEXT[],
        $4::UUID[],
        $5::TEXT[]
    ) WITH ORDINALITY AS o(constraints, content_hash, description, id, title, ord)
),
-- Look up existing slots and their versioned objects
existing_slots AS (
    SELECT
        vo.path_id,
        args.id as old_id,
        args.title,
        args.description,
        args.constraints,
        args.content_hash,
        args.ord
    FROM args
    JOIN rev.value_slots vs ON vs.id = args.id
    JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
),
-- Create new versioned objects with caller-supplied content_hash
new_versions_inserted AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        es.path_id,
        es.content_hash
    FROM existing_slots es
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
-- Include already-existing versioned objects (conflict hit — content reverted)
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
-- Create new value slots (skip if entity row already exists for this versioned_object)
new_slots AS (
    INSERT INTO rev.value_slots (versioned_object_id, title, description, constraints)
    SELECT
        av.id,
        es.title,
        es.description,
        COALESCE(es.constraints, '{}'::jsonb)
    FROM existing_slots es
    JOIN all_versions av ON av.path_id = es.path_id
    WHERE NOT EXISTS (SELECT 1 FROM rev.value_slots vs WHERE vs.versioned_object_id = av.id)
    RETURNING id, versioned_object_id, title, description, constraints, created_at
),
-- Include already-existing value slots (for reverted versions)
all_slots AS (
    SELECT ns.id, ns.versioned_object_id, ns.title, ns.description, ns.constraints, ns.created_at
    FROM new_slots ns
    UNION ALL
    SELECT vs.id, vs.versioned_object_id, vs.title, vs.description, vs.constraints, vs.created_at
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
    als.created_at AS "created_at!"
FROM all_slots als
JOIN all_versions av ON av.id = als.versioned_object_id
JOIN rev.object_paths op ON op.id = av.path_id
JOIN rev.value_slot_path_details vspd ON vspd.path_id = op.id
JOIN rev.value_kinds vk ON vk.id = vspd.value_kind_id
JOIN existing_slots es ON es.path_id = av.path_id
ORDER BY als.id;
