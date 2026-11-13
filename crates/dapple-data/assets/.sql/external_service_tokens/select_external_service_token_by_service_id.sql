SELECT
    est.id AS "id!",
    est.service_id,
    est.public_key_pem,
    est.key_algorithm,
    est.company_id,
    c.slug AS "company_slug!",
    c.revoked_at AS company_revoked_at,
    (
        SELECT MIN(r.revoked_at)
        FROM rev.external_service_token_revocations r
        WHERE r.external_service_token_id = est.id
    ) AS token_revoked_at,
                    esp.principal_id AS "principal_id!"
FROM rev.external_service_tokens est
JOIN rev.companies c ON c.id = est.company_id
JOIN rev.external_service_principals esp ON esp.external_service_token_id = est.id
WHERE est.service_id = $1::TEXT;
