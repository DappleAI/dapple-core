-- Migration 72: Seed the 'video' brief-output display hint
--
-- Migration 55 seeded image/text/markdown/list/checklist/table/fields/hidden and
-- migration 69 added 'audio'; neither could render a VIDEO asset. The
-- `dapple-learned-from-vod` guide produces a computed `dl_recap` (value_kind: asset,
-- an MP4 recap reel from `vod assemble`) whose whole point is to be watched, so it
-- needs a display hint a client renders as `<video controls>` (the video analog of
-- 'audio' -> <audio>). This appends 'video' to the closed vocabulary so
-- `:::brief-output { display: "video" }` materializes: the insert resolves
-- display_hint_key -> id via rev.brief_output_display_hints, and
-- BriefOutputDisplay::from_key maps it back on read.
--
-- Idempotent: 'video' may already exist on a re-applied/branch DB.
INSERT INTO rev.brief_output_display_hints (key)
VALUES ('video')
ON CONFLICT (key) DO NOTHING;
