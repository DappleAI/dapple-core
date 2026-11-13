-- Select inference profile configs for a profile
-- Resolves model_release and model_variant references
-- Parameters:
--   $1: inference_profile_id (UUID)

SELECT
    ipc.id,
    ipc.inference_profile_id,
    ipc.prompt_path_object_id,
    ipc.model_release_id,
    ipc.model_variant_id,
    ipc.priority,
    -- Model release fields (direct or via variant base)
    COALESCE(l.key, lv.key) AS lineage_key,
    COALESCE(p.key, pv.key) AS provider_key,
    COALESCE(mr.tier, mrv.tier) AS tier,
    COALESCE(mr.release, mrv.release) AS release,
    -- Variant-specific
    mv.key AS variant_key
FROM rev.inference_profile_configs ipc
-- Direct model release reference
LEFT JOIN rev.model_releases mr ON mr.id = ipc.model_release_id
LEFT JOIN rev.lineages l ON l.id = mr.lineage_id
LEFT JOIN rev.providers p ON p.id = mr.provider_id
-- Variant reference (resolves to base release)
LEFT JOIN rev.model_variants mv ON mv.id = ipc.model_variant_id
LEFT JOIN rev.model_releases mrv ON mrv.id = mv.base_release_id
LEFT JOIN rev.lineages lv ON lv.id = mrv.lineage_id
LEFT JOIN rev.providers pv ON pv.id = mrv.provider_id
WHERE ipc.inference_profile_id = $1
ORDER BY ipc.priority;
