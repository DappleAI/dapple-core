WITH token_lookup AS (
    SELECT id
    FROM web.api_tokens
    WHERE id = $1
),
objs AS (
    INSERT INTO web.api_token_revocations (api_token_id, reason)
    SELECT
        token_lookup.id,
        $2::TEXT
    FROM token_lookup
    RETURNING *
)
SELECT
    objs.id,
    objs.api_token_id,
    objs.reason,
    objs.revoked_at
FROM objs;
