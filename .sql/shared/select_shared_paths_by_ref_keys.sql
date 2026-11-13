WITH args AS (
    SELECT *
    FROM UNNEST($1::TEXT[]) WITH ORDINALITY AS o(ref_key, ord)
)
SELECT
    op.id,
    op.entity_type_id,
    op.scope,
    op.ref_key AS "ref_key!",
    et.key AS entity_type_key,
    op.created_at
FROM args
JOIN rev.object_paths op ON op.ref_key = args.ref_key
JOIN rev.entity_types et ON et.id = op.entity_type_id
WHERE op.ref_key IS NOT NULL
ORDER BY args.ord;
