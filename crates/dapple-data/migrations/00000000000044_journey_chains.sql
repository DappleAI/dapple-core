-- =============================================================================
-- Journey chains: predecessor_journey_id + guide_id on rev.journeys
-- =============================================================================
-- Replaces docs/plans/cross-journey-binding-inheritance.md. That plan
-- proposed copying bindings forward at first-dispatch creation; this one
-- introduces the missing primitive instead: an explicit predecessor link on
-- the journey itself. Bindings live where they were created; readers walk
-- the chain (journey-local first, then back through predecessor) to compute
-- the effective binding per value_slot_id.
--
-- Two columns, both nullable:
--
--   predecessor_journey_id UUID NULL — the journey this one continues from.
--     NULL = chain root. Three concurrent essays = three independent chains
--     rooted at three independent chapter-1 journeys.
--   guide_id UUID NULL — which specific guide version this journey serves.
--     A playbook can belong to multiple guides; without this column the
--     "which guide is this chain part of" question is ambiguous. Pinning
--     to a specific rev.guides row (not rev.guide_paths) means a chain
--     stays locked to one guide version — guide v2 doesn't migrate
--     in-flight chains.
--
-- Coherence constraint: if predecessor_journey_id is set, the predecessor's
-- guide_id must equal this row's guide_id. Each chain stays within one
-- guide version by construction. Enforced via a BEFORE INSERT/UPDATE
-- trigger because we cannot reference another row's column from a CHECK
-- (Postgres CHECK constraints cannot read other rows; subqueries are
-- disallowed).
--
-- No backfill: existing journeys keep NULL on both columns and behave
-- exactly as today. New journeys post-migration carry chain identity.
-- =============================================================================

ALTER TABLE rev.journeys
    ADD COLUMN predecessor_journey_id UUID NULL
        REFERENCES rev.journeys (id),
    ADD COLUMN guide_id UUID NULL
        REFERENCES rev.guides (id);

CREATE INDEX journeys_predecessor_journey_idx
    ON rev.journeys (predecessor_journey_id)
    WHERE predecessor_journey_id IS NOT NULL;

CREATE INDEX journeys_guide_idx
    ON rev.journeys (guide_id)
    WHERE guide_id IS NOT NULL;

-- Chain coherence: predecessor's guide_id must match this row's.
-- Implemented as a trigger because CHECK constraints can't query other
-- rows. A row with predecessor but no guide_id is rejected; a row with
-- guide_id but no predecessor is fine (chain root). A predecessor whose
-- guide_id differs from this row's is rejected.

CREATE OR REPLACE FUNCTION rev.journeys_check_chain_coherence()
RETURNS TRIGGER AS $$
DECLARE
    pred_guide_id UUID;
BEGIN
    IF NEW.predecessor_journey_id IS NULL THEN
        RETURN NEW;
    END IF;

    IF NEW.guide_id IS NULL THEN
        RAISE EXCEPTION
            'journey % has predecessor_journey_id but no guide_id; chains require a guide pin',
            NEW.id;
    END IF;

    SELECT guide_id INTO pred_guide_id
    FROM rev.journeys
    WHERE id = NEW.predecessor_journey_id;

    IF pred_guide_id IS DISTINCT FROM NEW.guide_id THEN
        RAISE EXCEPTION
            'journey %: predecessor %.guide_id (%) does not match this row''s guide_id (%); chains stay within one guide version',
            NEW.id, NEW.predecessor_journey_id, pred_guide_id, NEW.guide_id;
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER journeys_chain_coherence_trigger
    BEFORE INSERT OR UPDATE OF predecessor_journey_id, guide_id
    ON rev.journeys
    FOR EACH ROW
    EXECUTE FUNCTION rev.journeys_check_chain_coherence();
