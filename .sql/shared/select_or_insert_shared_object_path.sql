-- Upsert pattern: find existing object_path by ref_key, or create a new one.
-- Returns the existing row if ref_key already exists, otherwise inserts and returns the new row.
WITH existing AS (
    SELECT id, entity_type_id, scope, ref_key, created_at
    FROM rev.object_paths
    WHERE ref_key = $3
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = $1
),
new_path AS (
    INSERT INTO rev.object_paths (entity_type_id, scope, ref_key)
    SELECT
        (SELECT id FROM entity_type),
        $2,
        $3
    WHERE NOT EXISTS (SELECT 1 FROM existing)
    RETURNING id, entity_type_id, scope, ref_key, created_at
)
SELECT
    id AS "id!",
    entity_type_id AS "entity_type_id!",
    scope,
    ref_key AS "ref_key!",
    created_at AS "created_at!"
FROM existing
UNION ALL
SELECT
    id AS "id!",
    entity_type_id AS "entity_type_id!",
    scope,
    ref_key AS "ref_key!",
    created_at AS "created_at!"
FROM new_path;
