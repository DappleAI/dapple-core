-- Register a new external service token for a company.
--
-- `service_id` format and uniqueness are enforced by the Migration 34
-- CHECK + UNIQUE constraints; the handler maps the corresponding sqlx
-- errors to HTTP 400/409. The Migration 37 trigger auto-creates the
-- matching `rev.external_service_principals` row, so handlers do not need
-- to insert principals from application code.
--
-- `key_algorithm` defaults to 'Ed25519' at the table level; the schema
-- only supports Ed25519 today.
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
