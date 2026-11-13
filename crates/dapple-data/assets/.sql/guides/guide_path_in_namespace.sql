SELECT EXISTS (
    SELECT 1
    FROM rev.object_paths
    WHERE id = $2::UUID
      AND namespace_company_id = $1::UUID
      AND scope = 'external'
) AS in_namespace;
