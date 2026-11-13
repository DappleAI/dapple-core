-- Batch insert Lago event sync records and notify the worker
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[],
        $3::BIGINT[]
    ) WITH ORDINALITY AS o(lago_transaction_id, resource_id, resource_type_id, ord)
),
new_syncs AS (
    INSERT INTO rev.lago_event_sync (resource_type_id, resource_id, lago_transaction_id)
    SELECT args.resource_type_id, args.resource_id, args.lago_transaction_id
    FROM args
    ORDER BY args.ord
    RETURNING id, resource_type_id, resource_id, lago_transaction_id, created_at
),
notify AS (
    SELECT pg_notify('lago_event_sync', ns.id::text)
    FROM new_syncs ns
)
SELECT ns.id, ns.resource_type_id, ns.resource_id, ns.lago_transaction_id, ns.created_at
FROM new_syncs ns
ORDER BY ns.id;
