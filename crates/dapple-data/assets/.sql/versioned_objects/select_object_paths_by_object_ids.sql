WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    op.id,
    op.entity_type_id,
    et.key AS entity_type_key,
    op.created_at
FROM args
JOIN rev.object_paths op ON op.id = args.id
JOIN rev.entity_types et ON et.id = op.entity_type_id
ORDER BY op.id;
