-- Select pending (unsent) Lago event sync records
SELECT
    les.id,
    les.resource_type_id,
    les.resource_id,
    les.lago_transaction_id,
    les.retry_count,
    les.created_at
FROM rev.lago_event_sync les
WHERE les.sent_at IS NULL
  AND (les.next_retry_at IS NULL OR les.next_retry_at <= now())
ORDER BY les.created_at
LIMIT $1::BIGINT;
