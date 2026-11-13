-- True iff `journey_id` exists AND its `owner_company_id` matches the caller.
-- Used by external-API handlers to gate every journey-targeted operation
-- (list briefs, create dispatch, etc.).
SELECT EXISTS (
    SELECT 1
    FROM rev.journeys
    WHERE id = $2::UUID
      AND owner_company_id = $1::UUID
) AS in_namespace;
