
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[]
    ) WITH ORDINALITY AS o(binding_id, asset_dataset_extraction_id, ord)
),
inserted AS (
    INSERT INTO rev.dispatch_intent_binding_dataset_items
        (dispatch_intent_binding_id, asset_dataset_extraction_id)
    SELECT args.binding_id, args.asset_dataset_extraction_id
    FROM args
    ORDER BY args.ord
    RETURNING id, dispatch_intent_binding_id, asset_dataset_extraction_id, created_at
)
SELECT
    i.id,
    i.dispatch_intent_binding_id,
    i.asset_dataset_extraction_id,
    ade.envelope,
    i.created_at
FROM inserted i
JOIN rev.asset_dataset_extractions ade ON ade.id = i.asset_dataset_extraction_id
ORDER BY i.id;
