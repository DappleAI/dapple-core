SELECT
    id,
    slug,
    name,
    created_at,
    revoked_at
FROM rev.companies
WHERE
    $1::BOOL
    OR revoked_at IS NULL
ORDER BY id DESC;
