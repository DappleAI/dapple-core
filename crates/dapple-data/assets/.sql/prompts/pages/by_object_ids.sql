args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
),
prompt_page AS (
    SELECT p.id
    FROM args
    JOIN rev.prompts p ON p.id = args.id
)
