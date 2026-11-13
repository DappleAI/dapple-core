SELECT
    u.id,
    u.journey_id,
    u.asset_id,
    u.input_key,
    u.value_slot_id,
    u.file_name,
    a.mime_type,
    a.content_length,
    vo.content_hash,
    u.created_at
FROM rev.journey_asset_uploads u
JOIN rev.assets a ON a.id = u.asset_id
JOIN rev.versioned_objects vo ON vo.id = a.versioned_object_id
LEFT JOIN rev.journey_asset_uploads_removed r
    ON r.journey_asset_upload_id = u.id
WHERE u.journey_id = $1::UUID
  AND r.journey_asset_upload_id IS NULL
ORDER BY u.id;
