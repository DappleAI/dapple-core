WITH token AS (
    SELECT id
    FROM web.service_tokens
    WHERE id = $1
)
INSERT INTO web.service_token_revocations (service_token_id, reason)
SELECT token.id, $2
FROM token
RETURNING id, service_token_id, reason, revoked_at;
