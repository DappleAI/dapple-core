-- Select directive paths by their IDs
WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    op.id,
    dpd.name,
    op.created_at
FROM args
JOIN rev.object_paths op ON op.id = args.id
JOIN rev.directive_path_details dpd ON dpd.path_id = op.id
ORDER BY op.id;
