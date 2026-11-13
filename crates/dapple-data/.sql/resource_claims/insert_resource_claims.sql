-- Batch insert resource claims
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::JSONB[],
        $2::UUID[],
        $3::UUID[],
        $4::BIGINT[]
    ) WITH ORDINALITY AS o(claim_data, correlation_id, dispatch_task_attempt_id, resource_type_id, ord)
),
new_claims AS (
    INSERT INTO rev.resource_claims (resource_type_id, dispatch_task_attempt_id, correlation_id, claim_data)
    SELECT args.resource_type_id, args.dispatch_task_attempt_id, args.correlation_id, args.claim_data
    FROM args
    ORDER BY args.ord
    RETURNING id, resource_type_id, created_at
)
SELECT nc.id, nc.resource_type_id, nc.created_at
FROM new_claims nc
ORDER BY nc.id;
