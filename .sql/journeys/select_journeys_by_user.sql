WITH args AS (
    SELECT
        $1::UUID AS user_object_id
),
limited AS (
    SELECT objs.*
    FROM rev.journeys objs
    JOIN rev.user_details u ON u.user_id = objs.user_id
    JOIN args ON args.user_object_id = u.object_id
    ORDER BY objs.id DESC
    LIMIT $2 OFFSET $3
)
SELECT
    limited.id,
    limited.playbook_id,
    u.object_id AS user_id,
    limited.name,
    limited.created_at,
    -- Brief pack association fields (nullable from LEFT JOIN)
    jbp.id AS "journey_brief_pack_id?",
    bp.id AS "brief_pack_id?",
    -- Inference profile association fields (nullable from LEFT JOIN)
    jip.id AS "journey_inference_profile_id?",
    ip.id AS "inference_profile_id?"
FROM limited
JOIN rev.playbooks p ON limited.playbook_id = p.id
JOIN rev.user_details u ON u.user_id = limited.user_id
LEFT JOIN rev.journey_brief_packs jbp ON jbp.journey_id = limited.id
LEFT JOIN rev.brief_packs bp ON bp.id = jbp.brief_pack_id
LEFT JOIN rev.journey_inference_profiles jip ON jip.journey_id = limited.id
LEFT JOIN rev.inference_profiles ip ON ip.id = jip.inference_profile_id
ORDER by limited.id DESC, jbp.id, jip.id;
