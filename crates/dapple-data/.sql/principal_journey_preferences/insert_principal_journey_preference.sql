-- Append a single preference row for (principal_id, journey_id). Each
-- PATCH/DELETE on /api/journeys/{id} writes one row; reads always take the
-- latest via `DISTINCT ON (principal_id, journey_id) ORDER BY ..., id DESC`,
-- so we never UPDATE here.
--
-- Bind order (alphabetical): hidden_at=$1, journey_id=$2, name=$3,
-- principal_id=$4.
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
