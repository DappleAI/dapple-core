-- Select a single model release by lineage, tier, and optionally release version
-- Parameters:
--   $1: lineage_key (TEXT)
--   $2: tier (TEXT)
--   $3: release (TEXT, nullable)

SELECT
    mr.id,
    mr.lineage_id,
    l.key AS lineage_key,
    mr.provider_id,
    p.key AS provider_key,
    mr.tier,
    mr.release,
    mr.created_at
FROM rev.model_releases mr
JOIN rev.lineages l ON l.id = mr.lineage_id
JOIN rev.providers p ON p.id = mr.provider_id
WHERE l.key = $1
  AND mr.tier = $2
  AND (mr.release = $3 OR ($3 IS NULL AND mr.release IS NULL) OR $3 IS NULL)
LIMIT 1;
