args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
),
brief_page AS (
    SELECT b.id
    FROM args
    JOIN rev.briefs b ON b.id = args.id
)
