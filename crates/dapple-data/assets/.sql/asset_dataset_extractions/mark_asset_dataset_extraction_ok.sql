UPDATE rev.asset_dataset_extractions
SET status = 'ok',
    envelope = $2,
    table_count = $3,
    row_count = $4,
    completed_at = now()
WHERE id = $1
RETURNING
    id,
    asset_id,
    extractor_version,
    status,
    envelope,
    table_count,
    row_count,
    error_message,
    created_at,
    completed_at;
