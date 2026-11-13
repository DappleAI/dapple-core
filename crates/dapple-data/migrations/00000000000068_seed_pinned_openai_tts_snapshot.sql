-- Pin the OpenAI TTS snapshot. Adds the dated snapshot tier
-- `gpt-4o-mini-tts-2025-12-15` under lineage `tts` / provider `openai`, so a guide
-- `:::inference-profile` can pin `.tier` (and the matching engine-body `model`
-- string) to the exact snapshot instead of floating on the bare `gpt-4o-mini-tts`
-- slug. The team wants TTS version changes to be deliberate and visible rather
-- than silently tracking OpenAI's latest snapshot.
--
-- `tier` is the runtime OpenAI model id the conduit reads from `engine_body.model`;
-- it MUST equal the guide's `engine_body.model` (chk_one_model) — here both are
-- `gpt-4o-mini-tts-2025-12-15`. The floating-slug row (`gpt-4o-mini-tts`, migration
-- 067) is left in place for any caller that still wants auto-latest.
--
-- Idempotent via NOT EXISTS (a NULL `release` won't trip the UNIQUE, matching
-- migration 067's pattern).
INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, v.tier, NULL
FROM rev.lineages l, rev.providers p,
    (VALUES ('gpt-4o-mini-tts-2025-12-15')) AS v(tier)
WHERE l.key = 'tts' AND p.key = 'openai'
  AND NOT EXISTS (
      SELECT 1
      FROM rev.model_releases mr
      WHERE mr.lineage_id = l.id
        AND mr.provider_id = p.id
        AND mr.tier = v.tier
        AND mr.release IS NULL
  );
