-- Select all deprecation levels from the lookup table
SELECT
    id,
    key,
    description,
    created_at
FROM rev.deprecation_levels
ORDER BY id;
