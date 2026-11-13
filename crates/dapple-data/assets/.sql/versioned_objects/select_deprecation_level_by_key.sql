SELECT
    id,
    key,
    description,
    created_at
FROM rev.deprecation_levels
WHERE key = $1;
