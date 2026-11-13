-- Migration 92: record when a computed output produced a value that was EMPTY
--
-- A generate task that returns `{"choices": []}` was, until now, indistinguishable
-- from one that returned `{"choices": [...]}`. `extract_by_source_path` returns a
-- successful `Some(Array([]))`, the producer wrote a binding row, and every gate in
-- the system asks only `EXISTS (binding row)` — the typed item tables are never
-- joined. So the slot read as bound, the step read as done, and a pick whose
-- candidate source was empty became unsatisfiable with nothing anywhere able to say
-- so. The journey stalled on a pane that reported success.
--
-- Emptiness cannot be recovered downstream in SQL without duplicating the choice
-- unwrap logic (a candidate source is stored either as a bare array or as the whole
-- `{choices: [...]}` object, depending on the guide's source_path). The producer
-- already holds the extracted value and can answer it exactly once, here.
--
-- This column is the FACT, not the policy. An empty value is perfectly legal for
-- most slots; it is only fatal where a consumer requires at least one item, and
-- that judgement belongs to the gates that read this flag, not to the write.
--
-- DEFAULT false is correct for every existing row: operator writes are never
-- "produced empty" (they carry exactly what the operator submitted), and any
-- historical computed binding predates the distinction, so treating it as
-- non-empty preserves today's behaviour for journeys already in flight.

ALTER TABLE rev.dispatch_intent_bindings
    ADD COLUMN produced_empty BOOLEAN NOT NULL DEFAULT false;

COMMENT ON COLUMN rev.dispatch_intent_bindings.produced_empty IS
    'True when the computed-output producer extracted a value that was present but empty '
    '(empty array/object, null, blank string, or a multi output that routed zero items). '
    'Always false for operator-submitted bindings. Read by the journey gates to decide '
    'whether a dependent pick can ever be satisfied.';
