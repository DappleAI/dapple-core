-- Add the `timer` brief-input control type — a rehearsal stopwatch.
--
-- A `timer` control captures a DURATION in seconds: the operator hits Start,
-- reads the adjacent content aloud, hits Stop, and the elapsed seconds become the
-- field's numeric value (which stays hand-editable / re-timeable). It is the
-- time-centric presentation-builder's core input: each talking-point entry carries
-- a `say` text field the operator reads plus a `duration_seconds` `timer` field,
-- so a timeline's axis can reflect real rehearsed time and roll the total up
-- against the up-front length target (see the timeline `axis` config's
-- `duration_field` + `total_slot`). Like the other accumulator field controls it
-- carries no domain knowledge and is rendered by dapple-ui; used as a per-entry
-- `item_fields` control (which aren't catalog-validated), this seed row also makes
-- it valid as a top-level control for consistency.
INSERT INTO rev.brief_input_control_types (key) VALUES
    ('timer')
ON CONFLICT (key) DO NOTHING;
