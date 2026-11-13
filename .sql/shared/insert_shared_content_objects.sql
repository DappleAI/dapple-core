WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[],
        $3::TEXT[],
        $4::TEXT[]
    ) WITH ORDINALITY AS o(shared_path_id, target_path_id, object_kind, object_name, ord)
),
objs AS (
    INSERT INTO rev.shared_content_objects (id, shared_path_id, target_path_id, object_kind, object_name, created_at)
    SELECT
        gen_random_uuid(),
        args.shared_path_id,
        args.target_path_id,
        args.object_kind,
        args.object_name,
        now()
    FROM args
    ORDER BY args.ord
    RETURNING id, shared_path_id, target_path_id, object_kind, object_name, created_at
)
SELECT
    objs.id,
    objs.shared_path_id,
    objs.target_path_id,
    objs.object_kind,
    objs.object_name,
    objs.created_at
FROM objs
ORDER BY objs.id;
