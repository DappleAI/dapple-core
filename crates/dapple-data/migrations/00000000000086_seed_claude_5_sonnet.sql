-- Seed the Claude 5 (Sonnet) taxonomy so an `:::inference-profile` declaring
-- `.lineage = claude-5 .tier = sonnet` resolves to a rev.model_releases row during
-- guide materialization.
--
-- Without this, materialize_inference_profile_configs
-- (crates/dapple-prompt-api/src/handler/guide_ingest.rs) resolves neither a
-- model_release nor a model_variant and inserts an inference_profile_configs row
-- with both ids NULL, violating chk_one_model.
--
-- This is the taxonomy record, not the runtime model source: the conduit's
-- `AnthropicBackend::map_model` keys on (lineage, tier) and ignores `release`, and
-- the `(claude-5, sonnet)` arm maps to the inlined literal `claude-sonnet-5`
-- (rig 0.39 does not export a Sonnet 5 constant). Mirrors the claude-4-8 / opus
-- seed (migration 085): a seeded lineage + a NULL-release model_release.
--
-- Note the env-default (conduit `default_model_from_env`) intentionally stays on
-- claude-4-6 / sonnet; this lineage is used only by prompts that explicitly pin it
-- (run-the-10k's editorial section generators + refine).
--
-- Idempotent via ON CONFLICT (lineage key) and NOT EXISTS (a NULL `release` would
-- not trip the UNIQUE(lineage_id, provider_id, tier, release) ON CONFLICT, since
-- NULL <> NULL). The `anthropic` provider is already seeded (migration 002).

INSERT INTO rev.lineages (key, description)
VALUES ('claude-5', 'Claude 5 family (Sonnet)')
ON CONFLICT (key) DO NOTHING;

INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, 'sonnet', NULL
FROM rev.lineages l, rev.providers p
WHERE l.key = 'claude-5'
  AND p.key = 'anthropic'
  AND NOT EXISTS (
      SELECT 1
      FROM rev.model_releases mr
      WHERE mr.lineage_id = l.id
        AND mr.provider_id = p.id
        AND mr.tier = 'sonnet'
        AND mr.release IS NULL
  );
