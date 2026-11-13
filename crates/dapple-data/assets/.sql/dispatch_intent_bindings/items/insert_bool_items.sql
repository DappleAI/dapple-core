
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::BOOL[]
    ) WITH ORDINALITY AS o(binding_id, value, ord)
)
INSERT INTO rev.dispatch_intent_binding_bool_items (dispatch_intent_binding_id, value)
SELECT args.binding_id, args.value
FROM args
ORDER BY args.ord
RETURNING id, dispatch_intent_binding_id, value, created_at;
