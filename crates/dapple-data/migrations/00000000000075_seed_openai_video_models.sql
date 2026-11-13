-- Migration 75: Seed the OpenAI Sora video releases
--
-- The multi-timelined-video guide generates its pattern/foreground clips through
-- the new OpenAI Sora backend (conduit service/video/openai.rs, selected by
-- engine_body.provider = "openai"). As with every backend (see migrations
-- 057/058 for gpt-image and 066 for the Fal media engines), an
-- `:::inference-profile` must resolve to a rev.model_releases row during guide
-- materialization — without a seeded taxonomy row the materializer inserts an
-- inference_profile_configs row with both model ids NULL, violating
-- chk_one_model.
--
-- `tier` is the runtime model id the conduit reads from engine_body.model
-- ("sora-2" / "sora-2-pro") — the tier MUST equal the guide's engine_body.model.
--
-- Idempotent: model_releases via NOT EXISTS (a NULL `release` won't trip
-- UNIQUE(lineage_id, provider_id, tier, release) since NULL <> NULL, matching
-- 057/058/066). The `openai` provider is seeded in migration 002 and the
-- `video` lineage in 066.
INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, v.tier, NULL
FROM rev.lineages l, rev.providers p,
    (VALUES
        ('sora-2'),
        ('sora-2-pro')
    ) AS v(tier)
WHERE l.key = 'video' AND p.key = 'openai'
  AND NOT EXISTS (
      SELECT 1
      FROM rev.model_releases mr
      WHERE mr.lineage_id = l.id
        AND mr.provider_id = p.id
        AND mr.tier = v.tier
        AND mr.release IS NULL
  );
