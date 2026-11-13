-- Select a single entity type by key
SELECT
    id,
    key,
    description,
    created_at
FROM rev.entity_types
WHERE key = $1;
