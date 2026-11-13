-- ============================================================================
-- Migration 50 — Value-slot cardinality (single vs multi / keep-N)
-- ============================================================================
-- A value slot's answer was implicitly exactly one item. To support keep-N
-- curation (the operator selects a SUBSET of a runtime-generated candidate set,
-- or a static `checkbox` BriefUnit collects several choices, or a task produces
-- an array into a computed slot), cardinality becomes a first-class property of
-- the slot.
--
--   * cardinality = 'single' (default) -> exactly one item; today's behavior,
--     so every existing slot is unchanged. The runner delivers a scalar.
--   * cardinality = 'multi'            -> N typed items of the slot's own
--     value_kind, bounded by [min_selections, max_selections]. The runner
--     delivers an ARRAY to the consuming task.
--
-- These live on the VERSIONED row (rev.value_slots), alongside `constraints`,
-- so a cardinality change forks a new version via the content hash (the
-- ingest materializer folds them into hash_value_slot). value_kind_id stays in
-- rev.value_slot_path_details (path-level identity); arity is a versioned
-- property, like constraints.
--
-- min/max are NULL-as-unbounded. The candidate-source reference (which computed
-- slot supplies a dynamic pick's choices) is NOT a column here — it lives in
-- rev.brief_unit_input.control_spec JSONB as `candidate_source_value_slot_id`,
-- mirroring the existing `value_slot_id` key.
-- ============================================================================

ALTER TABLE rev.value_slots
    ADD COLUMN cardinality TEXT NOT NULL DEFAULT 'single'
        CHECK (cardinality IN ('single', 'multi')),
    ADD COLUMN min_selections INT
        CHECK (min_selections IS NULL OR min_selections >= 0),
    ADD COLUMN max_selections INT
        CHECK (max_selections IS NULL OR max_selections >= 1),
    -- min must not exceed max when both are set.
    ADD CONSTRAINT value_slots_selection_bounds_ck
        CHECK (
            min_selections IS NULL
            OR max_selections IS NULL
            OR min_selections <= max_selections
        ),
    -- selection bounds only mean anything for multi slots.
    ADD CONSTRAINT value_slots_single_no_bounds_ck
        CHECK (
            cardinality = 'multi'
            OR (min_selections IS NULL AND max_selections IS NULL)
        );
