WITH args AS (
    SELECT *
    FROM UNNEST($2::UUID[]) WITH ORDINALITY AS o(id, ord)
),
inference_profile_page AS (
    SELECT ip.id, args.ord
    FROM args
    JOIN rev.inference_profiles ip ON ip.id = args.id
    ORDER BY args.ord
    LIMIT $1::BIGINT OFFSET $3::BIGINT
)

SELECT
    ip.id AS "id!",
    ip.versioned_object_id AS "versioned_object_id!",
    vo.path_id AS "path_id!",
    ipd.name AS "name!",
    vo.content_hash AS "content_hash!",
    dl.key AS "deprecation_level_key?",
    ip.description,
    ip.created_at AS "created_at!",
        ipc.id AS "config_id?",
    ipc.prompt_path_object_id AS "config_prompt_path_id?",
    ipc.model_release_id AS "config_model_release_id?",
    ipc.model_variant_id AS "config_model_variant_id?",
    ipc.priority AS "config_priority?",
        COALESCE(l.key, lv.key) AS "config_lineage_key?",
    COALESCE(p.key, pv.key) AS "config_provider_key?",
    COALESCE(mr.tier, mrv.tier) AS "config_tier?",
    COALESCE(mr.release, mrv.release) AS "config_release?",
    mv.key AS "config_variant_key?"
FROM inference_profile_page ipp
JOIN rev.inference_profiles ip ON ip.id = ipp.id
JOIN rev.versioned_objects vo ON vo.id = ip.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.inference_profile_path_details ipd ON ipd.path_id = op.id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.inference_profile_configs ipc ON ipc.inference_profile_id = ip.id
LEFT JOIN rev.model_releases mr ON mr.id = ipc.model_release_id
LEFT JOIN rev.lineages l ON l.id = mr.lineage_id
LEFT JOIN rev.providers p ON p.id = mr.provider_id
LEFT JOIN rev.model_variants mv ON mv.id = ipc.model_variant_id
LEFT JOIN rev.model_releases mrv ON mrv.id = mv.base_release_id
LEFT JOIN rev.lineages lv ON lv.id = mrv.lineage_id
LEFT JOIN rev.providers pv ON pv.id = mrv.provider_id
ORDER BY ip.id, ipc.priority;

