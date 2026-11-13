-- True iff `directive_id` exists AND its path lives in `caller_company_id`'s
-- namespace. Used by external-API handlers to gate `GET /api/directives/{id}`.
SELECT EXISTS (
    SELECT 1
    FROM rev.directives d
    JOIN rev.versioned_objects vo ON vo.id = d.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    WHERE d.id = $2::UUID
      AND op.namespace_company_id = $1::UUID
) AS in_namespace;
