UPDATE rev.asset_extractions
SET status = 'running'
WHERE id = $1
  AND status = 'pending'
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
