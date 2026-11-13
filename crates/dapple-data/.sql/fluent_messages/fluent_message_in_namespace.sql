-- True iff `fluent_message_id` exists AND its path lives in `caller_company_id`'s
-- namespace. Used by external-API handlers to gate `GET /api/fluent-messages/{id}`.
SELECT EXISTS (
    SELECT 1
    FROM rev.fluent_messages fm
    JOIN rev.versioned_objects vo ON vo.id = fm.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    WHERE fm.id = $2::UUID
      AND op.namespace_company_id = $1::UUID
) AS in_namespace;
