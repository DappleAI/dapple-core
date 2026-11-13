SELECT
    est.id AS "id!",
    est.service_id,
    est.company_id,
    est.name,
    est.description,
    est.created_at,
    (
        SELECT MIN(r.revoked_at)
        FROM rev.external_service_token_revocations r
        WHERE r.external_service_token_id = est.id
    ) AS revoked_at,
    esp.principal_id AS "principal_id!"
FROM rev.external_service_tokens est
JOIN rev.external_service_principals esp ON esp.external_service_token_id = est.id
WHERE est.company_id = $1::UUID
ORDER BY est.id DESC;
