-- Seed the OpenAI text-to-speech taxonomy (generic `tts` lineage + OpenAI TTS
-- releases) so an `:::inference-profile` selecting an OpenAI TTS model resolves
-- to a rev.model_releases row during guide materialization
-- (materialize_inference_profile_configs) — exactly as migrations 057/058/066
-- seeded the OpenAI gpt-image and Fal media releases. Without a seeded taxonomy
-- row the materializer resolves neither a model_release nor a model_variant and
-- inserts an inference_profile_configs row with both ids NULL, violating
-- chk_one_model.
--
-- This replaces the removed VITS/ONNX TTS path. The conduit reaches these models
-- through rath-rs with OpenAI as the provider
-- (crates/dapple-prompt-conduit/src/service/tts/rath.rs). TTS routes by its maker
-- key (`tts`); an engine_body.provider = "openai" discriminator selects the rath
-- backend. `tier` is the runtime OpenAI model id the conduit reads from
-- engine_body.model (e.g. `gpt-4o-mini-tts`) — these are taxonomy records, not
-- the runtime model source. Add a row here for any additional OpenAI TTS model a
-- guide authors; the tier MUST equal that guide's engine_body.model.
--
-- The legacy `vits` lineage, `vits-*` engines, and `self-hosted` VITS releases
-- (migration 002/004) are left INERT — nothing references them once guides move
-- to the `tts` lineage, and `self-hosted`/maker `tts` are shared with
-- flan-t5/modernbert so they must remain.
--
-- Idempotent: lineage ON CONFLICT (key) DO NOTHING; model_releases via NOT EXISTS
-- (a NULL `release` won't trip UNIQUE(lineage_id, provider_id, tier, release)
-- since NULL <> NULL, matching 057/058/066). The `openai` provider is already
-- seeded (migration 002).

-- Generic text-to-speech lineage (distinct from the model-specific `vits`).
INSERT INTO rev.lineages (key, description) VALUES
    ('tts', 'Text-to-speech models')
ON CONFLICT (key) DO NOTHING;

-- OpenAI TTS releases under the `tts` lineage, reached via rath's OpenAI TtsClient.
INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, v.tier, NULL
FROM rev.lineages l, rev.providers p,
    (VALUES
        ('gpt-4o-mini-tts'),
        ('tts-1'),
        ('tts-1-hd')
    ) AS v(tier)
WHERE l.key = 'tts' AND p.key = 'openai'
  AND NOT EXISTS (
      SELECT 1
      FROM rev.model_releases mr
      WHERE mr.lineage_id = l.id
        AND mr.provider_id = p.id
        AND mr.tier = v.tier
        AND mr.release IS NULL
  );
