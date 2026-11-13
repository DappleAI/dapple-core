UPDATE rev.asset_dataset_extractions
SET status = 'failed',
    error_message = $2,
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
