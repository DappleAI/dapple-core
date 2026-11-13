-- Seed the canonical undated Claude 4.6 Sonnet release (the `claude-sonnet-4-6`
-- alias). Migration 4 seeded `claude-4-6 / anthropic / sonnet / 20250929`, but
-- `20250929` is the Sonnet 4.5 snapshot date carried over as a placeholder. The
-- current Anthropic convention for 4.6 is the *undated* alias (no date suffix),
-- which this taxonomy represents with a NULL release (same as the vits seed).
--
-- This row is cosmetic: the conduit's `AnthropicBackend::map_model` keys on
-- (lineage, tier) and ignores `release`, so `claude-4-6 / sonnet` already maps
-- to `claude-sonnet-4-6` regardless of which release row a profile references.
-- The existing `.../20250929` row is left in place (harmless). To point a guide's
-- inference profile at this NULL-release row, drop the `.release` attribute on
-- the `:::inference-profile` and re-ingest (a dapple-guides change).
--
-- Idempotent via NOT EXISTS (a NULL `release` would not trip the
-- UNIQUE(lineage_id, provider_id, tier, release) ON CONFLICT, since NULL <> NULL).
INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, 'sonnet', NULL
FROM rev.lineages l, rev.providers p
WHERE l.key = 'claude-4-6'
  AND p.key = 'anthropic'
  AND NOT EXISTS (
      SELECT 1
      FROM rev.model_releases mr
      WHERE mr.lineage_id = l.id
        AND mr.provider_id = p.id
        AND mr.tier = 'sonnet'
        AND mr.release IS NULL
  );
