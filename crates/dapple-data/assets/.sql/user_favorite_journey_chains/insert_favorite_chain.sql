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
chain_root AS (
    SELECT journey_id AS root_id
    FROM journey_chain
    ORDER BY depth DESC
    LIMIT 1
),
user_lookup AS (
    SELECT ud.user_id, ud.object_id
    FROM rev.user_details ud
    WHERE ud.object_id = $2::UUID
),
active_check AS (
    SELECT f.id
    FROM web.user_favorite_journey_chains f
    LEFT JOIN web.user_favorite_journey_chains_removed r
        ON r.user_favorite_journey_chains_id = f.id
    WHERE f.user_id = (SELECT user_id FROM user_lookup)
      AND f.chain_root_journey_id = (SELECT root_id FROM chain_root)
      AND r.user_favorite_journey_chains_id IS NULL
    ORDER BY f.id DESC
    LIMIT 1
),
inserted AS (
    INSERT INTO web.user_favorite_journey_chains (user_id, chain_root_journey_id)
    SELECT (SELECT user_id FROM user_lookup),
           (SELECT root_id FROM chain_root)
    WHERE NOT EXISTS (SELECT 1 FROM active_check)
      AND EXISTS (SELECT 1 FROM chain_root)
      AND EXISTS (SELECT 1 FROM user_lookup)
    RETURNING id, user_id, chain_root_journey_id, created_at
)
SELECT
    i.id,
    ul.object_id AS user_id,
    i.chain_root_journey_id,
    j.name AS chain_root_name,
    i.created_at
FROM inserted i
JOIN rev.journeys j ON j.id = i.chain_root_journey_id
JOIN user_lookup ul ON ul.user_id = i.user_id;
