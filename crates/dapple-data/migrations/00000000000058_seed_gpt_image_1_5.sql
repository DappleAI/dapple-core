-- Seed the gpt-image-1.5 release under the `image` lineage.
--
-- gpt-image-1.5 is the latest model in the "1" family (lineage 1 -> 1.5 -> 2).
-- Unlike gpt-image-2 (which rejects background "transparent" on the OpenAI-direct
-- surface), gpt-image-1.5 supports BOTH opaque and transparent generation, so the
-- wimmelbook guide uses it for every image profile: the opaque base render AND the
-- transparent -piece cutouts.
--
-- Like migration 057, this is the taxonomy record so that an
-- `:::inference-profile` declaring `.lineage = image .tier = gpt-image-1.5`
-- resolves to a rev.model_releases row during guide materialization. Without it,
-- materialize_inference_profile_configs (crates/dapple-prompt-api/src/handler/
-- guide_ingest.rs) resolves neither a model_release nor a model_variant and
-- inserts an inference_profile_configs row with both ids NULL, violating
-- chk_one_model. The conduit reads the runtime model id from engine_body.model,
-- so this release is the taxonomy record, not the runtime model source.
--
-- Idempotent via NOT EXISTS (a NULL `release` would not trip the
-- UNIQUE(lineage_id, provider_id, tier, release) ON CONFLICT, since NULL <> NULL).
-- The `image` lineage (migration 057) and `openai` provider (migration 002) are
-- already seeded.

INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, v.tier, NULL
FROM rev.lineages l, rev.providers p,
    (VALUES ('gpt-image-1.5')) AS v(tier)
WHERE l.key = 'image' AND p.key = 'openai'
  AND NOT EXISTS (
      SELECT 1
      FROM rev.model_releases mr
      WHERE mr.lineage_id = l.id
        AND mr.provider_id = p.id
        AND mr.tier = v.tier
        AND mr.release IS NULL
  );
