SELECT
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
    completed_at
FROM rev.asset_extractions
WHERE asset_id = $1
  AND extractor_kind = $2
  AND is_current = TRUE
ORDER BY extractor_version DESC
LIMIT 1;
