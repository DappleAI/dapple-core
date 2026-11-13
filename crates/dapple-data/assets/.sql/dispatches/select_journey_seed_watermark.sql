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

SELECT
    (SELECT count(*)
     FROM rev.dispatch_intent_bindings dib
     JOIN rev.dispatches d ON d.id = dib.dispatch_id
     WHERE d.journey_id IN (SELECT journey_id FROM journey_chain)
    ) AS "binding_count!",
    (SELECT count(*)
     FROM rev.dispatch_task_attempt_events ev
     JOIN rev.dispatch_task_attempts a ON a.id = ev.attempt_id
     JOIN rev.dispatch_steps ds ON ds.id = a.dispatch_step_id
     JOIN rev.dispatch_stages dst ON dst.id = ds.dispatch_stage_id
     JOIN rev.dispatches d ON d.id = dst.dispatch_id
     WHERE d.journey_id = $1::UUID
       AND ev.phase_code = 'ok'
    ) AS "ok_event_count!";
