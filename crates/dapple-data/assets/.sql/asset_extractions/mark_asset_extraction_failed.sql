UPDATE rev.asset_extractions
SET status = 'failed',
    error_message = $2,
    completed_at = now()
WHERE id = $1
RETURNING
    id,
    asset_id,
    extractor_kind,
    extractor_version,
    status,
    text_length,
    page_count,
    metadata,
    error_message,
    is_current,
    created_at,
    completed_at;
