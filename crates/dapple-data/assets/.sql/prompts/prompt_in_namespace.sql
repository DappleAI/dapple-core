SELECT EXISTS (
    SELECT 1
    FROM rev.prompts p
    JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    WHERE p.id = $2::UUID
      AND op.namespace_company_id = $1::UUID
) AS in_namespace;
