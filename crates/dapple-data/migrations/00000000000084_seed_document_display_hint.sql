-- Migration 84: Seed the 'document' brief-output display hint
--
-- Migration 55 seeded the base display vocabulary, 69/72/76 added
-- audio/video/file. 'file' renders a download card only — fine for artifacts
-- opened elsewhere (OTIO timelines, editable SVGs), but a PDF report produced
-- by `document render` (conduit /render-document) is meant to be READ on the
-- pane. 'document' is the read-in-place sibling of 'file': the client renders
-- an inline viewer (<iframe> on the asset content endpoint) plus a download
-- link. The insert resolves display_hint_key -> id via
-- rev.brief_output_display_hints, and BriefOutputDisplay::from_key maps it
-- back on read.
--
-- Idempotent: 'document' may already exist on a re-applied/branch DB.
INSERT INTO rev.brief_output_display_hints (key)
VALUES ('document')
ON CONFLICT (key) DO NOTHING;
