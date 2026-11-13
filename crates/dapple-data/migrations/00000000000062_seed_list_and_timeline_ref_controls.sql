-- Add the `list-ref` and `timeline-ref` brief-input control types.
--
-- A ref control is a typed POINTER from one accumulator to another. Used as a
-- per-entry field inside a `repeater`/`timeline` `item_fields` schema, a ref
-- field names a SEPARATE value slot (`ref_slot`) that holds its own accumulator;
-- the operator drills into the referenced slot's list/timeline rather than
-- editing an embedded sub-list. The sub-list therefore LIVES ONCE in the
-- referenced slot and is shared/reused across every entry (and every place) that
-- references it — nesting is a slot reference, not embedded JSON. The client's
-- pane stack navigates the reference chain (slot A --ref--> slot B --ref--> ...),
-- pushing a new pane per hop; a single shared accumulator renderer draws each
-- pane. `list-ref` points at a flat `repeater` slot; `timeline-ref` at a
-- `timeline` (repeater-with-axis) slot.
--
-- Backend cost: ingest resolves the field's `ref_slot` NAME to the referenced
-- slot's `value_slot_id` (the same name->id resolution as `candidate_source`)
-- and bakes it into the field's resolved `item_fields` entry as
-- `ref_value_slot_id`, which already rides through `/steps` in the `item_fields`
-- JSON. The referenced slot is a normal brief input exposed on some step, so the
-- client resolves its render spec from the journey's `/steps` by that id — no
-- inlining, no new schema. These seed rows only make the two keys valid for use
-- as a TOP-LEVEL control (item-field controls are not catalog-validated); the
-- resolver + pane stack live in dapple-ui.
INSERT INTO rev.brief_input_control_types (key) VALUES
    ('list-ref'),
    ('timeline-ref')
ON CONFLICT (key) DO NOTHING;
