UPDATE rev.lago_event_sync
SET
    sent_at = CASE WHEN $5::BOOLEAN THEN now() ELSE sent_at END,
    lago_response_status = $3::INTEGER,
    lago_response_body = $2::JSONB,
    retry_count = CASE WHEN $5::BOOLEAN THEN retry_count ELSE retry_count + 1 END,
    next_retry_at = $4::TIMESTAMPTZ
WHERE id = $1::UUID
RETURNING id;
