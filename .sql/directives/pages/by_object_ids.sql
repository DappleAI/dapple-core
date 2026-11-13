args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
),
directive_page AS (
    SELECT d.id
    FROM args
    JOIN rev.directives d ON d.id = args.id
)
