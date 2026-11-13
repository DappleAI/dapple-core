-- Select drafts by IDs
WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    d.id,
    d.asset_kind_id,
    ak.key as asset_kind_key,
    d.mime_type,
    d.created_at
FROM args
JOIN rev.drafts d ON d.id = args.id
JOIN rev.asset_kinds ak ON ak.id = d.asset_kind_id
ORDER BY d.id;
