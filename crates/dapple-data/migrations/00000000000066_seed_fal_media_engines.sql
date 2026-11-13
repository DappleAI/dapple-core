-- Seed the Fal media-generation taxonomy (provider + image/video releases) so an
-- `:::inference-profile` selecting a Fal model resolves to a rev.model_releases row
-- during guide materialization (materialize_inference_profile_configs) — exactly as
-- migrations 057/058 seeded the OpenAI gpt-image releases, and as the deleted
-- migration 064 seeded the (now-removed) Stability/LTX direct backends. Without a
-- seeded taxonomy row the materializer resolves neither a model_release nor a
-- model_variant and inserts an inference_profile_configs row with both ids NULL,
-- violating chk_one_model.
--
-- The conduit reaches these models through rath-rs with Fal as the provider
-- (crates/dapple-prompt-conduit/src/service/{image,video}/rath.rs). Image and video
-- route by their maker key (`image`/`video`); a new engine_body.provider = "fal"
-- discriminator selects the rath backend. `tier` is the runtime Fal model slug the
-- conduit reads from engine_body.model (e.g. `fal-ai/layer-diffusion`) — these are
-- taxonomy records, not the runtime model source. Add a row here for any additional
-- Fal slug a guide authors; the tier MUST equal that guide's engine_body.model.
--
-- Idempotent: provider/lineage ON CONFLICT (key) DO NOTHING; model_releases via
-- NOT EXISTS (a NULL `release` won't trip UNIQUE(lineage_id, provider_id, tier,
-- release) since NULL <> NULL, matching 057/058).

-- Fal provider. `openai` is seeded in migration 002.
INSERT INTO rev.providers (key, display_name) VALUES
    ('fal', 'Fal')
ON CONFLICT (key) DO NOTHING;

-- Video lineage. The `image` lineage is seeded in 057. First seeded by the deleted
-- migration 064; re-seeded here so a fresh database still gets it.
INSERT INTO rev.lineages (key, description) VALUES
    ('video', 'Video generation models')
ON CONFLICT (key) DO NOTHING;

-- Fal image releases under the shared `image` lineage. LayerDiffuse (SDXL + alpha)
-- for native transparent pieces; flux for opaque backdrops.
INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, v.tier, NULL
FROM rev.lineages l, rev.providers p,
    (VALUES
        ('fal-ai/layer-diffusion'),
        ('fal-ai/flux/schnell'),
        ('fal-ai/flux/dev')
    ) AS v(tier)
WHERE l.key = 'image' AND p.key = 'fal'
  AND NOT EXISTS (
      SELECT 1
      FROM rev.model_releases mr
      WHERE mr.lineage_id = l.id
        AND mr.provider_id = p.id
        AND mr.tier = v.tier
        AND mr.release IS NULL
  );

-- Fal video releases under the `video` lineage (LTX-Video on Fal's queue).
INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, v.tier, NULL
FROM rev.lineages l, rev.providers p,
    (VALUES
        ('fal-ai/ltx-video'),
        ('fal-ai/ltx-2/text-to-video'),
        ('fal-ai/ltx-2.3/text-to-video')
    ) AS v(tier)
WHERE l.key = 'video' AND p.key = 'fal'
  AND NOT EXISTS (
      SELECT 1
      FROM rev.model_releases mr
      WHERE mr.lineage_id = l.id
        AND mr.provider_id = p.id
        AND mr.tier = v.tier
        AND mr.release IS NULL
  );
