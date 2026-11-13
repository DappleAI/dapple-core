SELECT
    id,
    asset_id,
    extractor_version,
    status,
    envelope,
    table_count,
    row_count,
    error_message,
    created_at,
    completed_at
FROM rev.asset_dataset_extractions
WHERE asset_id = $1
ORDER BY id DESC
LIMIT 1;
