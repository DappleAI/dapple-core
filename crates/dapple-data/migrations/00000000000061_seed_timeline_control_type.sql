-- Add the `timeline` brief-input control type.
--
-- A `timeline` control is a `repeater` with an axis. The operator authors an
-- ordered run of entry records accumulating into a `cardinality:"multi"` value
-- slot, where each entry carries a stable `id`, a `parent_id` (null at the root)
-- that nests it under another entry, a `kind` (span | cue), and an `anchor`
-- ({ mode: sequence | absolute | relative, ref, offset }). The whole forest --
-- the root entries and every nested level -- is one FLAT array in the slot; the
-- tree is reconstructed by grouping on `parent_id`, so nesting is a uuid
-- reference, not embedded JSON and not a separate store. Like `repeater`, the
-- per-entry content fields are declared by the guide (`item_fields`) and rendered
-- through the existing form controls, so the control carries no domain knowledge,
-- and accumulation rides the existing multi-json + latest-wins resolver
-- (depth ASC, bound_at DESC). The control type is a free string validated
-- dynamically at guide ingest, so this seed row is the ONLY schema change the
-- timeline needs; the axis renderer + recursion live in dapple-ui.
INSERT INTO rev.brief_input_control_types (key) VALUES
    ('timeline')
ON CONFLICT (key) DO NOTHING;
