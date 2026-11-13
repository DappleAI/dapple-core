-- Add the `duration` brief-input control type — a typed length entry.
--
-- A `duration` control takes a span of time as two boxes, minutes and seconds,
-- and stores the total canonically in SECONDS (min × 60 + sec). It is the
-- up-front partner to the `timer` stopwatch (migration 063): where `timer`
-- CAPTURES a rehearsed length, `duration` lets the operator TYPE a target length
-- unambiguously. The time-centric presentation-builder uses it for "how long is
-- the talk" so the target lives in the SAME unit (seconds) as each rehearsed
-- talking point, removing the earlier minutes-in / seconds-everywhere-else split
-- (the axis reads the bound value directly as `totalSeconds`, no ×60 fold).
--
-- Like the other numeric controls it carries no domain knowledge and is rendered
-- by dapple-ui; it is a top-level control over a `number` value slot (top-level
-- controls ARE catalog-validated at ingest, so this seed is what makes a
-- `control: "duration"` brief-input ingestable).
INSERT INTO rev.brief_input_control_types (key) VALUES
    ('duration')
ON CONFLICT (key) DO NOTHING;
