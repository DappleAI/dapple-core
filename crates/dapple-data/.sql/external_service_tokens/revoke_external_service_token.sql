-- Revoke an external service token by inserting a row into the append-only
-- `rev.external_service_token_revocations` log. The external extractor
-- refuses any token whose external_service_token_id appears in this table.
--
-- Lookup is done via a CTE so the INSERT is a no-op when the token id is
-- unknown; the handler maps `None` to HTTP 404.
WITH token_lookup AS (
    SELECT id
    FROM rev.external_service_tokens
    WHERE id = $1::UUID
)
INSERT INTO rev.external_service_token_revocations (
    external_service_token_id,
    reason
)
SELECT
    token_lookup.id,
    $2::TEXT
FROM token_lookup
RETURNING
    id,
    external_service_token_id,
    revoked_at,
    revoked_by_user_id,
    reason;
