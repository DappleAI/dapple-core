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
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        es.path_id,
        es.content_hash
    FROM existing_slots es
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
-- Create new value slots
new_slots AS (
    INSERT INTO rev.value_slots (versioned_object_id, title, description, constraints)
    SELECT
        nv.id,
        es.title,
        es.description,
        COALESCE(es.constraints, '{}'::jsonb)
    FROM existing_slots es
    JOIN new_versions nv ON nv.path_id = es.path_id
    RETURNING id, versioned_object_id, title, description, constraints, created_at
)
SELECT
    ns.id,
    ns.versioned_object_id,
    op.id as path_id,
    vspd.name,
    vk.key as value_kind_key,
    nv.content_hash,
    ns.title,
    ns.description,
    ns.constraints,
    ns.created_at
FROM new_slots ns
JOIN new_versions nv ON nv.id = ns.versioned_object_id
JOIN rev.object_paths op ON op.id = nv.path_id
JOIN rev.value_slot_path_details vspd ON vspd.path_id = op.id
JOIN rev.value_kinds vk ON vk.id = vspd.value_kind_id
JOIN existing_slots es ON es.path_id = nv.path_id
ORDER BY ns.id;
