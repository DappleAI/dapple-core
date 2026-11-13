-- Add the `conversation` brief-input control type (the forward conversation
-- builder).
--
-- A `conversation` control is a `repeater` (a growing list over a
-- `cardinality:"multi"` slot) in FORWARD mode: instead of proposing a finished
-- list to prune and reorder, it GROWS the list one record at a time. After each
-- record is added, dapple-ui re-runs the generate step with the list-so-far in
-- the dispatch INTENT (the `repeater/suggest` route) so the next options FOLLOW
-- the chain, and the author never reorders. It carries no domain knowledge: the
-- per-item fields are the guide's (`item_fields`), and everything else
-- (candidate_source, refine_input) matches `repeater` exactly. The control type
-- is a free string validated dynamically at guide ingest against this lookup
-- (`rev.brief_input_control_types`), so this seed row is the ONLY schema change
-- the forward builder needs; the renderer lives in dapple-ui (which maps the
-- `conversation` control_type onto the repeater slide with `forward: true`).
--
-- Follows the same one-row precedent as the `repeater` (migration 060),
-- `timeline` (061), `timer` (063), and `duration` (065) control-type seeds.
INSERT INTO rev.brief_input_control_types (key) VALUES
    ('conversation')
ON CONFLICT (key) DO NOTHING;
