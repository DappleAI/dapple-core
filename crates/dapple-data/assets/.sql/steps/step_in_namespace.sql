SELECT EXISTS (
    SELECT 1
    FROM rev.steps s
    JOIN rev.versioned_objects vo ON vo.id = s.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    WHERE s.id = $2::UUID
      AND op.namespace_company_id = $1::UUID
) AS in_namespace;
