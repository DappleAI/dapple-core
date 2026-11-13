-- Migration 69: Seed the 'audio' brief-output display hint
--
-- The display vocabulary (migration 55) covered image/text/markdown/list/
-- checklist/table/fields/hidden but had no way to render an AUDIO asset. Crosstalk
-- produces a computed `conv_audio` (value_kind: asset) whose whole point is to be
-- heard, so it needs a display hint that a client renders as an <audio controls>
-- player (the audio analog of 'image' -> <img>). This appends 'audio' to the
-- closed vocabulary so `:::brief-output { display: "audio" }` materializes: the
-- insert resolves display_hint_key -> id via rev.brief_output_display_hints, and
-- BriefOutputDisplay::from_key maps it back on read.
--
-- Idempotent: 'audio' may already exist on a re-applied/branch DB.
INSERT INTO rev.brief_output_display_hints (key)
VALUES ('audio')
ON CONFLICT (key) DO NOTHING;
