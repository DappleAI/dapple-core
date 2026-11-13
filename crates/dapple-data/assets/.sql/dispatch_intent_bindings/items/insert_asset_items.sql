
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[]
    ) WITH ORDINALITY AS o(binding_id, asset_id, ord)
),
inserted AS (
    INSERT INTO rev.dispatch_intent_binding_asset_items (dispatch_intent_binding_id, asset_id)
    SELECT args.binding_id, args.asset_id
    FROM args
    ORDER BY args.ord
    RETURNING id, dispatch_intent_binding_id, asset_id, created_at
)
SELECT
    i.id,
    i.dispatch_intent_binding_id,
    i.asset_id,
    i.created_at
FROM inserted i
JOIN rev.assets a ON a.id = i.asset_id
ORDER BY i.id;
