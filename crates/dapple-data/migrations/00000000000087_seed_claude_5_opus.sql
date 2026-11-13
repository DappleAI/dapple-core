-- Seed the Claude 5 (Opus) taxonomy so an `:::inference-profile` declaring
-- `.lineage = claude-5 .tier = opus` resolves to a rev.model_releases row during
-- guide materialization.
--
-- The `claude-5` lineage is already seeded (migration 086, Sonnet 5); this adds the
-- `opus` tier under the same family. Without the model_release,
-- materialize_inference_profile_configs (crates/dapple-prompt-api/src/handler/guide_ingest.rs)
-- resolves neither a model_release nor a model_variant and fails ingest with
-- "references unknown model 'claude-5/opus' (add a seed migration)".
--
-- This is the taxonomy record, not the runtime model source: the conduit's
-- `AnthropicBackend::map_model` keys on (lineage, tier) and ignores `release`, and
-- the new `(claude-5, opus)` arm maps to the inlined literal `claude-opus-5`
-- (rig 0.39 does not export an Opus 5 constant). Mirrors the claude-5 / sonnet
-- seed (migration 086): a NULL-release model_release under the existing lineage.
--
-- The env-default (conduit `default_model_from_env`) intentionally stays on
-- claude-4-6 / sonnet; this (lineage, tier) is used only by prompts that explicitly
-- pin it (run-the-10k's five judgment prompts move here from claude-4-8 / opus).
--
-- Idempotent via NOT EXISTS (a NULL `release` would not trip the
-- UNIQUE(lineage_id, provider_id, tier, release) ON CONFLICT, since NULL <> NULL).
-- The `anthropic` provider (migration 002) and the `claude-5` lineage (migration 086)
-- are already seeded.

UPDATE rev.lineages
SET description = 'Claude 5 family (Sonnet, Opus)'
WHERE key = 'claude-5';

INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, 'opus', NULL
FROM rev.lineages l, rev.providers p
WHERE l.key = 'claude-5'
  AND p.key = 'anthropic'
  AND NOT EXISTS (
      SELECT 1
      FROM rev.model_releases mr
      WHERE mr.lineage_id = l.id
        AND mr.provider_id = p.id
        AND mr.tier = 'opus'
        AND mr.release IS NULL
  );
