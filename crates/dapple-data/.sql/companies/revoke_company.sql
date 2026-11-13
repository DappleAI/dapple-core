-- Mark a company revoked. The Migration 31 BEFORE-UPDATE trigger forbids
-- mutating `slug`, but `revoked_at` is freely updatable. Returns the
-- updated row; handler maps `None` to HTTP 404.
UPDATE rev.companies
SET revoked_at = COALESCE(revoked_at, now())
WHERE id = $1::UUID
RETURNING
    id,
    slug,
    name,
    created_at,
    revoked_at;
