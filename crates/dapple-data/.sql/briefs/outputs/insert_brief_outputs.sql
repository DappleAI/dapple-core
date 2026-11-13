WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[],
        $3::TEXT[],
        $4::TEXT[],
        $5::TEXT[],
        $6::JSONB[]
    ) WITH ORDINALITY AS o(brief_id, value_slot_id, unit_key, source_kind_key, source_path, transform, ord)
),
value_slot_refs AS (
    SELECT
        args.ord,
        vs.id AS value_slot_id,
        vspd.name AS value_slot_name,
        args.brief_id,
        args.unit_key,
        args.source_kind_key,
        args.source_path,
        args.transform
    FROM args
    JOIN rev.value_slots vs ON vs.id = args.value_slot_id
    JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.value_slot_path_details vspd ON vspd.path_id = op.id
),
objs AS (
    INSERT INTO rev.brief_outputs (
        brief_id,
        value_slot_id,
        unit_key,
        source_kind_id,
        source_path,
        transform,
        created_at
    )
    SELECT
        vsr.brief_id,
        vsr.value_slot_id,
        vsr.unit_key,
        sk.id,
        vsr.source_path,
        COALESCE(vsr.transform, '{}'::jsonb),
        now()
    FROM value_slot_refs vsr
    JOIN rev.brief_output_source_kinds sk ON sk.key = vsr.source_kind_key
    ORDER BY vsr.ord
    RETURNING id, brief_id, value_slot_id, unit_key, source_kind_id, source_path, transform, created_at
)
SELECT
    objs.id,
    objs.brief_id,
    objs.unit_key,
    objs.value_slot_id,
    vsr.value_slot_name,
    sk.key AS source_kind_key,
    objs.source_path,
    objs.transform,
    objs.created_at
FROM objs
JOIN value_slot_refs vsr ON vsr.brief_id = objs.brief_id AND vsr.value_slot_id = objs.value_slot_id
JOIN rev.brief_output_source_kinds sk ON sk.id = objs.source_kind_id
ORDER BY objs.id;
