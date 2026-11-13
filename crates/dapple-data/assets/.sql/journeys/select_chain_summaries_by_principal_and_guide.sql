WITH RECURSIVE
roots AS (
    SELECT j.id AS root_id, j.name AS root_name, j.created_at AS root_created_at
    FROM rev.journeys j
    WHERE j.predecessor_journey_id IS NULL
      AND j.principal_id = $2::UUID
      AND j.guide_id = $1::UUID
),
descendants(journey_id, root_id, depth) AS (
    SELECT r.root_id, r.root_id, 0 FROM roots r
    UNION ALL
    SELECT j.id, d.root_id, d.depth + 1
    FROM descendants d
    JOIN rev.journeys j ON j.predecessor_journey_id = d.journey_id
    WHERE d.depth < 50
),
leaves AS (
    SELECT d.journey_id AS leaf_id, d.root_id, d.depth
    FROM descendants d
    WHERE NOT EXISTS (
        SELECT 1 FROM rev.journeys child
        WHERE child.predecessor_journey_id = d.journey_id
    )
),
viewer_prefs AS (
    SELECT DISTINCT ON (pjp.journey_id)
        pjp.journey_id,
        pjp.hidden_at
    FROM rev.principal_journey_preferences pjp
    WHERE pjp.principal_id = $2::UUID
    ORDER BY pjp.journey_id, pjp.id DESC
)
SELECT
    l.root_id      AS "root_id!",
    rj.name        AS "root_name!",
    rj.created_at  AS "root_created_at!",
    l.leaf_id      AS "leaf_id!",
    l.depth        AS "depth!",
    j.playbook_id  AS "leaf_playbook_id!",
    j.name         AS "leaf_name!",
    j.created_at   AS "leaf_created_at!"
FROM leaves l
JOIN rev.journeys j  ON j.id = l.leaf_id
JOIN rev.journeys rj ON rj.id = l.root_id
LEFT JOIN viewer_prefs root_prefs ON root_prefs.journey_id = l.root_id
LEFT JOIN viewer_prefs leaf_prefs ON leaf_prefs.journey_id = l.leaf_id
WHERE root_prefs.hidden_at IS NULL
  AND leaf_prefs.hidden_at IS NULL
ORDER BY rj.created_at DESC, j.created_at DESC;
