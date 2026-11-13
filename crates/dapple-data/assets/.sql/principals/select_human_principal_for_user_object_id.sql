SELECT
    hp.principal_id AS "principal_id!"
FROM rev.user_details ud
JOIN rev.human_principals hp ON hp.user_id = ud.user_id
WHERE ud.object_id = $1::UUID;
