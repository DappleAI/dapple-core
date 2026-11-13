INSERT INTO rev.companies (slug, name)
VALUES ($2::TEXT, $1::TEXT)
RETURNING
    id,
    slug,
    name,
    created_at,
    revoked_at;
