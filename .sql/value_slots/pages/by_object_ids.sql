args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
),
value_slot_page AS (
    SELECT vs.id
    FROM args
    JOIN rev.value_slots vs ON vs.id = args.id
)
