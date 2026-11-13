UPDATE rev.companies
SET revoked_at = COALESCE(revoked_at, now())
WHERE id = $1::UUID
RETURNING
    id,
    slug,
    name,
    created_at,
    revoked_at;
