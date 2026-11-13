-- Select fluent message paths by IDs using unified versioning
WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    op.id,
    fmpd.name,
    fmpd.description,
    op.created_at
FROM args
JOIN rev.object_paths op ON op.id = args.id
JOIN rev.fluent_message_path_details fmpd ON fmpd.path_id = op.id
ORDER BY op.id;
