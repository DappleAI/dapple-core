WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::TEXT[],
        $3::TEXT[]
    ) WITH ORDINALITY AS o(asset_id, file_name, input_key, ord)
),
journey AS (
    SELECT j.id
    FROM rev.journeys j
    WHERE j.id = $4::UUID
),
objs AS (
    INSERT INTO rev.journey_asset_uploads
        (journey_id, asset_id, input_key, value_slot_id, file_name)
    SELECT journey.id, args.asset_id, args.input_key, vs.id, args.file_name
    FROM args
    CROSS JOIN journey
    LEFT JOIN rev.value_slots vs ON vs.id::text = lower(args.input_key)
    ORDER BY args.ord
    RETURNING id, journey_id, asset_id, input_key, value_slot_id, file_name, created_at
)
SELECT
    o.id,
    o.journey_id,
    o.asset_id,
    o.input_key,
    o.value_slot_id,
    o.file_name,
    a.mime_type,
    a.content_length,
    vo.content_hash,
    o.created_at
FROM objs o
JOIN rev.assets a ON a.id = o.asset_id
JOIN rev.versioned_objects vo ON vo.id = a.versioned_object_id
ORDER BY o.id;
