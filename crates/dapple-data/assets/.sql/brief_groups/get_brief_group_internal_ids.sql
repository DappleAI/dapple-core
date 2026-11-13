
WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    bg.id
FROM args
JOIN rev.brief_groups bg ON bg.id = args.id
ORDER BY args.ord;
