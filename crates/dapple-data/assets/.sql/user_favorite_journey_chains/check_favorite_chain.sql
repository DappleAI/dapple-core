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
)
SELECT
    (SELECT root_id FROM chain_root) AS "chain_root_journey_id?",
    EXISTS (
        SELECT 1
        FROM web.user_favorite_journey_chains f
        JOIN rev.user_details ud ON ud.user_id = f.user_id
        LEFT JOIN web.user_favorite_journey_chains_removed r
            ON r.user_favorite_journey_chains_id = f.id
        WHERE ud.object_id = $2::UUID
          AND f.chain_root_journey_id = (SELECT root_id FROM chain_root)
          AND r.user_favorite_journey_chains_id IS NULL
    )::bool AS "is_favorited!";
