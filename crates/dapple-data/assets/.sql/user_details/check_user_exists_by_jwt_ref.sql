SELECT
    ud.user_id,
    ud.object_id AS id
FROM rev.user_details ud
WHERE
    (ud.object_id = $2 AND $2 IS NOT NULL)
    OR (ud.user_id = $1 AND $1 IS NOT NULL);
