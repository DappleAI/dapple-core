-- Add the generic `repeater` brief-input control type.
--
-- A `repeater` control lets a journey operator author a GROWING list of records
-- one at a time (generate -> pick -> edit -> append), accumulating into a
-- `cardinality:"multi"` value slot. It is fully generic: the per-item fields are
-- declared by the guide (`item_fields`) and rendered through the existing form
-- controls, so the control carries no domain knowledge. The list grows by
-- read-modify-write: each "add" is its own dispatch whose task re-emits the FULL
-- list as a computed output, and latest-wins resolution (depth ASC, bound_at
-- DESC) surfaces the newest/longest. The control type is a free string validated
-- dynamically at guide ingest, so this seed row is the ONLY schema change the
-- accumulator needs; the renderer lives in dapple-ui.
--
-- This supersedes the guide-flavored `list_builder` control (migration 059):
-- accumulation was always generic, so the control is now a generic list-input.
-- The `list_builder` row is left in place (harmless; older guide versions may
-- still reference it) but no current guide or client uses it.
INSERT INTO rev.brief_input_control_types (key) VALUES
    ('repeater')
ON CONFLICT (key) DO NOTHING;
