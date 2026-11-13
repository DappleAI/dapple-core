-- =============================================================================
-- Stage-level journey forking: fork_stage_id on rev.journeys
-- =============================================================================
-- Migration 44 made journey chaining a whole-journey primitive: a successor
-- inherits its predecessor's ENTIRE binding pool via the live chain walk.
-- That grain predates the stages-are-panes model. This migration adds the
-- fork point:
--
--   fork_stage_id UUID NULL — a rev.playbook_stages row belonging to the
--     PREDECESSOR's playbook. When set, the chain walk crossing this edge
--     only admits predecessor-owned bindings whose producing dispatch
--     belongs to a stage with ord < fork stage ord ("fork AT stage N" =
--     redo N; inherit strictly before N). NULL = plain whole-journey chain
--     edge, exactly today's behavior.
--
-- The filter is edge-local: it applies only to the predecessor's own rows.
-- Bindings the predecessor itself inherited from deeper ancestors flow
-- through a forked edge untouched (each edge carries its own optional
-- filter). Stage completion is NOT consulted anywhere — forking is
-- provenance-scoped, not completion-gated.
--
-- Coherence: fork_stage_id requires predecessor_journey_id, and the stage
-- must belong to the predecessor's playbook (ord comparisons are only
-- meaningful within one playbook). Trigger, not CHECK, because the rule
-- reads other rows.
--
-- No backfill: existing journeys keep NULL and behave exactly as today.
-- =============================================================================

ALTER TABLE rev.journeys
    ADD COLUMN fork_stage_id UUID NULL
        REFERENCES rev.playbook_stages (id);

CREATE INDEX journeys_fork_stage_idx
    ON rev.journeys (fork_stage_id)
    WHERE fork_stage_id IS NOT NULL;

CREATE OR REPLACE FUNCTION rev.journeys_check_fork_coherence()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.fork_stage_id IS NULL THEN
        RETURN NEW;
    END IF;

    IF NEW.predecessor_journey_id IS NULL THEN
        RAISE EXCEPTION
            'journey % has fork_stage_id but no predecessor_journey_id; a fork point requires a predecessor',
            NEW.id;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM rev.playbook_stages ps
        JOIN rev.journeys pj ON pj.id = NEW.predecessor_journey_id
        WHERE ps.id = NEW.fork_stage_id
          AND ps.playbook_id = pj.playbook_id
    ) THEN
        RAISE EXCEPTION
            'journey %: fork_stage_id % is not a stage of predecessor %''s playbook',
            NEW.id, NEW.fork_stage_id, NEW.predecessor_journey_id;
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER journeys_fork_coherence_trigger
    BEFORE INSERT OR UPDATE OF fork_stage_id, predecessor_journey_id
    ON rev.journeys
    FOR EACH ROW
    EXECUTE FUNCTION rev.journeys_check_fork_coherence();
