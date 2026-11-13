-- Internal fetch: returns the raw attempt row by primary key without
-- any LATERAL join to events. Used by cache write-through and publisher
-- code paths where we just need the attempt's stored fields. The
-- public-facing select_by_object_id and select_by_dispatch_object_id
-- queries do the LATERAL-join enrichment that surfaces `error_detail`.
SELECT
    id,
    dispatch_step_id,
    task_id,
    status,
    context_data,
    context_metadata,
    NULL::text AS error_detail,
    created_at
FROM rev.dispatch_task_attempts
WHERE id = $1::uuid;
