args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
),
asset_page AS (
    SELECT a.id
    FROM args
    JOIN rev.assets a ON a.id = args.id
)
