
WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    op.id,
    bpd.name,
    op.created_at
FROM args
JOIN rev.object_paths op ON op.id = args.id
JOIN rev.brief_path_details bpd ON bpd.path_id = op.id
ORDER BY op.id;
