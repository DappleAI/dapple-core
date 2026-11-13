WITH args AS (
    SELECT
        $3::UUID AS owner_company_id,
        $4::UUID AS viewer_principal_id
),
viewer_prefs AS (
    SELECT DISTINCT ON (pjp.journey_id)
        pjp.journey_id,
        pjp.name AS name_override,
        pjp.hidden_at
    FROM rev.principal_journey_preferences pjp, args
    WHERE pjp.principal_id = args.viewer_principal_id
    ORDER BY pjp.journey_id, pjp.id DESC
),
limited AS (
    SELECT objs.*
    FROM rev.journeys objs
    JOIN args ON args.owner_company_id = objs.owner_company_id
    LEFT JOIN viewer_prefs vp ON vp.journey_id = objs.id
    WHERE vp.hidden_at IS NULL
    ORDER BY objs.id DESC
    LIMIT $1 OFFSET $2
)
SELECT
    limited.id,
    limited.playbook_id,
    limited.principal_id,
    u.object_id AS "user_id?",
    COALESCE(vp.name_override, limited.name) AS "name!",
    limited.created_at,
    vp.hidden_at AS "hidden_at?",
        jbp.id AS "journey_brief_pack_id?",
    bp.id AS "brief_pack_id?",
        jip.id AS "journey_inference_profile_id?",
    ip.id AS "inference_profile_id?",
    limited.guide_id AS "guide_id?",
    limited.predecessor_journey_id AS "predecessor_journey_id?",
    limited.fork_stage_id AS "fork_stage_id?"
FROM limited
JOIN rev.playbooks p ON limited.playbook_id = p.id
LEFT JOIN viewer_prefs vp ON vp.journey_id = limited.id
LEFT JOIN rev.user_details u ON u.user_id = limited.user_id
LEFT JOIN rev.journey_brief_packs jbp ON jbp.journey_id = limited.id
LEFT JOIN rev.brief_packs bp ON bp.id = jbp.brief_pack_id
LEFT JOIN rev.journey_inference_profiles jip ON jip.journey_id = limited.id
LEFT JOIN rev.inference_profiles ip ON ip.id = jip.inference_profile_id
ORDER by limited.id DESC, jbp.id, jip.id;
