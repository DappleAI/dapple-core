-- Seed the Claude 4.8 (Opus) taxonomy so an `:::inference-profile` declaring
-- `.lineage = claude-4-8 .tier = opus` resolves to a rev.model_releases row during
-- guide materialization.
--
-- Without this, materialize_inference_profile_configs
-- (crates/dapple-prompt-api/src/handler/guide_ingest.rs) resolves neither a
-- model_release nor a model_variant and inserts an inference_profile_configs row
-- with both ids NULL, violating chk_one_model.
--
-- This is the taxonomy record, not the runtime model source: the conduit's
-- `AnthropicBackend::map_model` keys on (lineage, tier) and ignores `release`, and
-- the `(claude-4-8, opus) | (_, opus)` arm already maps to the rig constant
-- CLAUDE_OPUS_4_8 (the literal `claude-opus-4-8`). Mirrors the vits / image seeds:
-- a seeded lineage + a NULL-release model_release.
--
-- Idempotent via ON CONFLICT (lineage key) and NOT EXISTS (a NULL `release` would
-- not trip the UNIQUE(lineage_id, provider_id, tier, release) ON CONFLICT, since
-- NULL <> NULL). The `anthropic` provider is already seeded (migration 002).

INSERT INTO rev.lineages (key, description)
VALUES ('claude-4-8', 'Claude 4.8 family (Opus)')
ON CONFLICT (key) DO NOTHING;

INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, 'opus', NULL
FROM rev.lineages l, rev.providers p
WHERE l.key = 'claude-4-8'
  AND p.key = 'anthropic'
  AND NOT EXISTS (
      SELECT 1
      FROM rev.model_releases mr
      WHERE mr.lineage_id = l.id
        AND mr.provider_id = p.id
        AND mr.tier = 'opus'
        AND mr.release IS NULL
  );
