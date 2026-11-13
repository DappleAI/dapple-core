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
