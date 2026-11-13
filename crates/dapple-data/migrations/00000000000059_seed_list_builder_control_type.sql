-- Add the `list_builder` brief-input control type.
--
-- A `list_builder` control lets a journey operator author a GROWING list one
-- item at a time (generate -> pick -> refine -> append), accumulating into a
-- `cardinality:"multi"` value slot. The list grows by read-modify-write: each
-- "add" is its own dispatch whose task re-emits the FULL list as a computed
-- output, and latest-wins resolution (depth ASC, bound_at DESC) surfaces the
-- newest/longest. The control type is a free string validated dynamically at
-- guide ingest, so this seed row is the ONLY schema change the accumulator
-- needs; the add/refine/done renderer lives in dapple-ui. (Existing controls:
-- text, textarea, select, radio, checkbox, number, date, file.)
INSERT INTO rev.brief_input_control_types (key) VALUES
    ('list_builder')
ON CONFLICT (key) DO NOTHING;
