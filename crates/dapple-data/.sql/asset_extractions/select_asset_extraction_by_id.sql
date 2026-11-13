-- Select a single asset extraction by id.
-- Parameters:
--   $1: id (UUID)
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
WHERE id = $1;
