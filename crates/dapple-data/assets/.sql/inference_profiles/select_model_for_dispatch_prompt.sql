
WITH prompt_path AS (
    SELECT vo.path_id
    FROM rev.prompts pmt
    JOIN rev.versioned_objects vo ON vo.id = pmt.versioned_object_id
    WHERE pmt.id = $2::UUID
)
SELECT
    COALESCE(l.key, lv.key) AS lineage_key,
    COALESCE(p.key, pv.key) AS provider_key,
    COALESCE(mr.tier, mrv.tier) AS tier,
    COALESCE(mr.release, mrv.release) AS release
FROM rev.dispatches d
JOIN rev.journey_inference_profiles jip ON jip.journey_id = d.journey_id
JOIN rev.inference_profiles ip ON ip.id = jip.inference_profile_id
JOIN rev.versioned_objects ipvo ON ipvo.id = ip.versioned_object_id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
    FROM rev.deprecation_events de
    WHERE de.versioned_object_id = ipvo.id
    ORDER BY de.created_at DESC
    LIMIT 1
) ld ON TRUE
JOIN rev.inference_profile_configs ipc ON ipc.inference_profile_id = ip.id
JOIN prompt_path pp ON pp.path_id = ipc.prompt_path_object_id
LEFT JOIN rev.model_releases mr ON mr.id = ipc.model_release_id
LEFT JOIN rev.lineages l ON l.id = mr.lineage_id
LEFT JOIN rev.providers p ON p.id = mr.provider_id
LEFT JOIN rev.model_variants mv ON mv.id = ipc.model_variant_id
LEFT JOIN rev.model_releases mrv ON mrv.id = mv.base_release_id
LEFT JOIN rev.lineages lv ON lv.id = mrv.lineage_id
LEFT JOIN rev.providers pv ON pv.id = mrv.provider_id
WHERE d.id = $1::UUID
  AND ld.to_level_id IS NULL
  AND COALESCE(l.key, lv.key) IS NOT NULL
ORDER BY ipc.priority ASC, ipvo.created_at DESC, ipvo.id DESC
LIMIT 1;
