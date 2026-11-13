args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
),
fluent_message_page AS (
    SELECT fm.id
    FROM args
    JOIN rev.fluent_messages fm ON fm.id = args.id
)
