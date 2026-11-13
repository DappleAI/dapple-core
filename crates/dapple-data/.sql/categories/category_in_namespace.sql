-- True iff `category_id` exists AND its path lives in `caller_company_id`'s
-- namespace. Used by external-API handlers to gate `GET /api/categories/{id}`.
SELECT EXISTS (
    SELECT 1
    FROM rev.categories c
    JOIN rev.versioned_objects vo ON vo.id = c.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    WHERE c.id = $2::UUID
      AND op.namespace_company_id = $1::UUID
) AS in_namespace;
