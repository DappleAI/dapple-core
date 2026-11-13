SELECT EXISTS (
    SELECT 1
    FROM rev.journeys
    WHERE id = $2::UUID
      AND owner_company_id = $1::UUID
) AS in_namespace;
