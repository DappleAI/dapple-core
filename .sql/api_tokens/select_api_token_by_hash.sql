-- Look up API token by hash for authentication validation
-- Returns user details needed for AuthContext
SELECT
    t.id,
    ud.object_id AS user_id,
    t.name,
    t.expires_at,
    -- Get the earliest revocation timestamp (if any)
    (
        SELECT MIN(r.revoked_at)
        FROM web.api_token_revocations r
        WHERE r.api_token_id = t.id
    ) AS revoked_at,
    -- Admin flag derived from sparse table (presence = admin)
    -- The ! suffix tells sqlx this is non-nullable despite the LEFT JOIN
    COALESCE(a.api_token_id IS NOT NULL, false) AS "is_admin!",
    -- User's roles from web.user_roles (defaults to ['user'] if none assigned)
    COALESCE(
        (SELECT array_agg(ur.role) FROM web.user_roles ur WHERE ur.user_id = t.user_id),
        ARRAY['user']::text[]
    ) AS "user_roles!"
FROM web.api_tokens t
JOIN rev.user_details ud ON ud.user_id = t.user_id
LEFT JOIN web.admin_api_tokens a ON a.api_token_id = t.id
WHERE t.token_hash = $1;
