
WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    fm.id
FROM args
JOIN rev.fluent_messages fm ON fm.id = args.id
ORDER BY args.ord;
