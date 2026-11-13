SELECT EXISTS (
    SELECT 1
    FROM rev.playbook_stages ps
    WHERE ps.id = $2::UUID
      AND ps.playbook_id = $1::UUID
) AS "belongs!";
