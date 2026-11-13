WITH RECURSIVE
journey_chain(journey_id, depth, fork_cutoff_ord) AS (
    SELECT $1::UUID, 0, NULL::INTEGER
    UNION ALL
    SELECT j.predecessor_journey_id, jc.depth + 1, ps.ord
    FROM journey_chain jc
    JOIN rev.journeys j ON j.id = jc.journey_id
    LEFT JOIN rev.playbook_stages ps ON ps.id = j.fork_stage_id
    WHERE j.predecessor_journey_id IS NOT NULL
      AND jc.depth < 50
)

,
viewer_prefs AS (
    SELECT DISTINCT ON (pjp.journey_id)
        pjp.journey_id,
        pjp.hidden_at
    FROM rev.principal_journey_preferences pjp
    WHERE pjp.principal_id = $2::UUID
    ORDER BY pjp.journey_id, pjp.id DESC
)
SELECT
    j.id           AS "id!",
    j.playbook_id  AS "playbook_id!",
    j.name         AS "name!",
    j.created_at   AS "created_at!",
    j.principal_id AS "principal_id!",
    j.owner_company_id        AS "owner_company_id!",
    j.predecessor_journey_id  AS "predecessor_journey_id?",
    j.guide_id     AS "guide_id?",
    j.fork_stage_id AS "fork_stage_id?",
    jc.depth       AS "depth!",
    vp.hidden_at   AS "hidden_at?"
FROM journey_chain jc
JOIN rev.journeys j ON j.id = jc.journey_id
LEFT JOIN viewer_prefs vp ON vp.journey_id = j.id
ORDER BY jc.depth DESC;
