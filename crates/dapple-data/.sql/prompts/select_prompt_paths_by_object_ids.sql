-- Select prompt paths by IDs using unified versioning
-- Parameters:
--   $1: ids (UUID[])

WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    op.id,
    ppd.name,
    op.created_at
FROM args
JOIN rev.object_paths op ON op.id = args.id
JOIN rev.prompt_path_details ppd ON ppd.path_id = op.id
ORDER BY op.id;
