SELECT
    id,
    slug,
    name,
    created_at,
    revoked_at
FROM rev.companies
WHERE slug = $1::TEXT;
