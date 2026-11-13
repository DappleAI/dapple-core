-- Add the `table` brief-input control type.
--
-- A `table` is a `repeater` rendered as an aligned grid: each accumulated
-- record is a ROW, and the guide-declared `item_fields` are the COLUMNS, laid
-- out on shared column tracks so the operator can scan down a column or across
-- a row (the repeater/timeline card stack cannot align values across records).
-- Everything else is the repeater data model unchanged -- candidate_source,
-- item_fields, refine_input, finalize/refine and multi-json + latest-wins
-- accumulation -- so per-column presentation (width, alignment) rides the
-- generic item_fields/attrs passthrough and the control carries no domain
-- knowledge. The control type is a free string validated dynamically at guide
-- ingest, so this seed row is the ONLY schema change the table needs; the grid
-- renderer lives in dapple-ui as a layout mode on the repeater path.
INSERT INTO rev.brief_input_control_types (key) VALUES
    ('table')
ON CONFLICT (key) DO NOTHING;
