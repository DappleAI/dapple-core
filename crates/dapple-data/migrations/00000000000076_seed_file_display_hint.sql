-- Migration 76: Seed the 'file' brief-output display hint
--
-- Migration 55 seeded image/text/markdown/list/checklist/table/fields/hidden,
-- 69 added 'audio', and 72 added 'video'; none can surface a DOWNLOADABLE
-- document artifact. The multi-timelined-video guide produces an OTIO timeline
-- document and an editable reveal SVG whose whole point is to be pulled down
-- and opened elsewhere, so they need a display hint a client renders as a
-- download card (icon + title + download anchor to /api/assets/{id}/content)
-- rather than an inline player/dump. This appends 'file' to the closed
-- vocabulary so `:::brief-output { display: "file" }` materializes: the insert
-- resolves display_hint_key -> id via rev.brief_output_display_hints, and
-- BriefOutputDisplay::from_key maps it back on read.
--
-- Idempotent: 'file' may already exist on a re-applied/branch DB.
INSERT INTO rev.brief_output_display_hints (key)
VALUES ('file')
ON CONFLICT (key) DO NOTHING;
