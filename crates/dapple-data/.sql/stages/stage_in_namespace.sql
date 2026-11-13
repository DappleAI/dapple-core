-- True iff `stage_id` exists AND its path's `namespace_company_id` matches
-- `caller_company_id`. Used by external-API handlers to gate access to a
-- single stage (customers can only read stages whose path lives in their
-- namespace).
SELECT EXISTS (
    SELECT 1
    FROM rev.stages s
    JOIN rev.versioned_objects vo ON vo.id = s.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    WHERE s.id = $2::UUID
      AND op.namespace_company_id = $1::UUID
) AS in_namespace;
