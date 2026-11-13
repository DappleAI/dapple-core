SELECT
    id,
    slug,
    name,
    created_at,
    revoked_at
FROM rev.companies
WHERE id = $1::UUID;
