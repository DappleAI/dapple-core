INSERT INTO rev.principal_journey_preferences (
    principal_id,
    journey_id,
    name,
    hidden_at
)
VALUES (
    $4::UUID,
    $2::UUID,
    $3::TEXT,
    $1::TIMESTAMPTZ
)
RETURNING
    id,
    principal_id,
    journey_id,
    name,
    hidden_at,
    created_at;
