UPDATE rev.asset_dataset_extractions
SET status = 'running'
WHERE id = $1
  AND status = 'pending'
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
