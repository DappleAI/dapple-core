INSERT INTO rev.external_service_tokens (
    service_id,
    company_id,
    name,
    description,
    public_key_pem
)
VALUES (
    $5::TEXT,
    $1::UUID,
    $3::TEXT,
    $2::TEXT,
    $4::TEXT
)
RETURNING
    id,
    service_id,
    company_id,
    name,
    description,
    public_key_pem,
    key_algorithm,
    created_at,
    created_by_user_id;
