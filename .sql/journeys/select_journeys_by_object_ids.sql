WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
),
limited AS (
    SELECT objs.*
    FROM rev.journeys objs
    JOIN args ON args.id = objs.id
    ORDER BY objs.id
    LIMIT $2 OFFSET $3
)
SELECT
    limited.id,
    limited.playbook_id,
    ud.object_id AS user_id,
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
JOIN rev.user_details ud ON limited.user_id = ud.user_id
LEFT JOIN rev.journey_brief_packs jbp ON jbp.journey_id = limited.id
LEFT JOIN rev.brief_packs bp ON bp.id = jbp.brief_pack_id
LEFT JOIN rev.journey_inference_profiles jip ON jip.journey_id = limited.id
LEFT JOIN rev.inference_profiles ip ON ip.id = jip.inference_profile_id
ORDER BY limited.id, jbp.id, jip.id;
