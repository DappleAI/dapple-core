-- Transition an asset extraction to 'ok' state with extracted metadata.
-- Parameters:
--   $1: id (UUID)
--   $2: text_length (BIGINT)
--   $3: page_count (INTEGER)
--   $4: metadata (JSONB)
UPDATE rev.asset_extractions
SET status = 'ok',
    text_length = $2,
    page_count = $3,
    metadata = $4,
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
