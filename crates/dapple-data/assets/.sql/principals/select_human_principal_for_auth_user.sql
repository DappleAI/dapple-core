SELECT
    hp.principal_id AS "principal_id!"
FROM rev.human_principals hp
WHERE hp.user_id = $1::BIGINT;
