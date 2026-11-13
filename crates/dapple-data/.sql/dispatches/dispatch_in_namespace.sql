-- True iff `dispatch_id` exists AND its `owner_company_id` matches the caller.
-- Used by external-API handlers to gate every dispatch-targeted operation
-- (post bindings, get briefs, poll status).
SELECT EXISTS (
    SELECT 1
    FROM rev.dispatches
    WHERE id = $2::UUID
      AND owner_company_id = $1::UUID
) AS in_namespace;
