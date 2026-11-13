-- Migration 40: Per-principal preferences for journeys
--
-- `rev.journeys` rows are immutable session-instances of a playbook. Each
-- viewer (principal) gets to overlay their own preferences on top — a
-- display-name override and a soft-hide timestamp — via append-only rows
-- in `rev.principal_journey_preferences`. Each PATCH/DELETE on the journey
-- endpoint writes a new preferences row; reads resolve the latest row per
-- (principal_id, journey_id) using `DISTINCT ON ... ORDER BY ..., id DESC`.
--
-- Whose preferences apply: the *viewer's* principal. If Alice (a human
-- principal) and the acme service (an external_service_principal)
-- both touch the same journey, each sees their own overrides; neither
-- mutates the journey row.

CREATE TABLE rev.principal_journey_preferences (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    principal_id UUID NOT NULL REFERENCES rev.principals (id),
    journey_id UUID NOT NULL REFERENCES rev.journeys (id),
    -- Per-principal display name. NULL means "use rev.journeys.name".
    name TEXT NULL,
    -- Set when the principal soft-hides the journey from their own list
    -- views via DELETE /api/journeys/{id}. NULL means visible.
    hidden_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Latest-wins read pattern:
--   SELECT DISTINCT ON (principal_id, journey_id) name, hidden_at
--   FROM rev.principal_journey_preferences
--   WHERE principal_id = $1 [AND journey_id = ANY($2)]
--   ORDER BY principal_id, journey_id, id DESC;
-- The index supports both the predicate and the DESC ordering directly.
CREATE INDEX principal_journey_preferences_latest_idx
    ON rev.principal_journey_preferences (principal_id, journey_id, id DESC);
