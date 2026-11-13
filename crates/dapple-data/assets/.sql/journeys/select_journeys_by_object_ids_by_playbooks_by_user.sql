WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],          $2::UUID[]       ) WITH ORDINALITY AS o(id, playbook_id, ord)
),
limited AS (
    SELECT objs.*
    FROM rev.journeys objs
    JOIN rev.playbooks p ON objs.playbook_id = p.id
    JOIN args ON args.id = objs.id AND args.playbook_id = p.id
    ORDER BY objs.id
    LIMIT $3 OFFSET $4
),
viewer_prefs AS (
    SELECT DISTINCT ON (pjp.journey_id)
        pjp.journey_id,
        pjp.name AS name_override,
        pjp.hidden_at
    FROM rev.principal_journey_preferences pjp
    JOIN limited ON limited.id = pjp.journey_id
    WHERE pjp.principal_id = $5::UUID
    ORDER BY pjp.journey_id, pjp.id DESC
)
SELECT
    limited.id,
    limited.playbook_id,
    limited.principal_id,
    ud.object_id AS "user_id?",
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
LEFT JOIN rev.user_details ud ON limited.user_id = ud.user_id
LEFT JOIN rev.journey_brief_packs jbp ON jbp.journey_id = limited.id
LEFT JOIN rev.brief_packs bp ON bp.id = jbp.brief_pack_id
LEFT JOIN rev.journey_inference_profiles jip ON jip.journey_id = limited.id
LEFT JOIN rev.inference_profiles ip ON ip.id = jip.inference_profile_id
ORDER BY limited.id, jbp.id, jip.id;
