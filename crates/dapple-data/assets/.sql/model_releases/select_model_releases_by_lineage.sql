
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
ORDER BY mr.tier, mr.release NULLS FIRST;
