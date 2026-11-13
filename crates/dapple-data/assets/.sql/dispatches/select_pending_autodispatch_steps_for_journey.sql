
WITH
journey_steps AS (
    SELECT ps.id AS playbook_stage_id, ss.id AS stage_step_id, ss.step_id,
           ps.ord AS stage_ord, ss.ord AS step_ord
    FROM rev.journeys j
    JOIN rev.playbook_stages ps ON ps.playbook_id = j.playbook_id
    JOIN rev.stage_steps ss ON ss.stage_id = ps.stage_id
    WHERE j.id = $1::UUID
),
step_tasks AS (
    SELECT jst.playbook_stage_id, jst.stage_step_id, jst.stage_ord, jst.step_ord,
           stk.id AS step_task_id, stk.ord AS task_ord
    FROM journey_steps jst
    JOIN rev.step_tasks stk ON stk.step_id = jst.step_id
),
all_overlays AS (
    SELECT st.stage_step_id, o.value_slot_path_id, o.visibility, 1 AS priority
    FROM rev.playbook_stage_overlays o
    JOIN step_tasks st ON st.playbook_stage_id = o.playbook_stage_id
    UNION ALL
    SELECT st.stage_step_id, o.value_slot_path_id, o.visibility, 2 AS priority
    FROM rev.stage_step_overlays o
    JOIN step_tasks st ON st.stage_step_id = o.stage_step_id
    UNION ALL
    SELECT st.stage_step_id, o.value_slot_path_id, o.visibility, 3 AS priority
    FROM rev.step_task_overlays o
    JOIN step_tasks st ON st.step_task_id = o.step_task_id
),
ranked AS (
    SELECT DISTINCT ON (stage_step_id, value_slot_path_id)
        stage_step_id, value_slot_path_id, visibility
    FROM all_overlays
    ORDER BY stage_step_id, value_slot_path_id, priority DESC
),
exposed AS (
    SELECT stage_step_id, value_slot_path_id
    FROM ranked
    WHERE visibility = 'expose'
),
operator_steps AS (
        SELECT DISTINCT e.stage_step_id
    FROM exposed e
    WHERE EXISTS (
        SELECT 1
        FROM rev.brief_unit_input bui
        JOIN rev.value_slots vs
            ON bui.control_spec ? 'value_slot_id'
           AND vs.id = (bui.control_spec ->> 'value_slot_id')::UUID
        JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
        WHERE vo.path_id = e.value_slot_path_id
    )
),
computed_steps AS (
        SELECT DISTINCT e.stage_step_id
    FROM exposed e
    WHERE EXISTS (
        SELECT 1
        FROM rev.brief_outputs bo
        JOIN rev.brief_output_source_kinds bosk
            ON bosk.id = bo.source_kind_id AND bosk.key = 'computed'
        JOIN rev.value_slots vs ON vs.id = bo.value_slot_id
        JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
        WHERE vo.path_id = e.value_slot_path_id
    )
),
attempt_latest AS (
                            SELECT DISTINCT ON (ev.attempt_id)
        ev.attempt_id,
        ev.phase_code,
        ev.context_metadata ->> 'error_class' AS error_class
    FROM rev.dispatch_task_attempt_events ev
    ORDER BY ev.attempt_id, ev.id DESC
),
step_dispatches AS (
            SELECT ds.stage_step_id, d.id AS dispatch_id, ds.id AS dispatch_step_id
    FROM rev.dispatches d
    JOIN rev.dispatch_stages dst ON dst.dispatch_id = d.id
    JOIN rev.dispatch_steps ds ON ds.dispatch_stage_id = dst.id
    WHERE d.journey_id = $1::UUID
),
step_dispatch_verdicts AS (
                                                SELECT
        sd.stage_step_id,
        sd.dispatch_id,
        CASE
            WHEN count(al.attempt_id) = 0          THEN NULL
            WHEN bool_or(NOT dp.terminal)          THEN NULL
            WHEN bool_or(al.phase_code = 'unsafe') THEN 'unsafe'
            WHEN bool_or(al.phase_code = 'failed'
                     AND al.error_class = 'deterministic')
                                                   THEN 'failed_deterministic'
            WHEN bool_or(al.phase_code = 'failed') THEN 'failed'
            ELSE 'ok'
        END AS verdict
    FROM step_dispatches sd
    LEFT JOIN rev.dispatch_task_attempts a ON a.dispatch_step_id = sd.dispatch_step_id
    LEFT JOIN attempt_latest al ON al.attempt_id = a.id
    LEFT JOIN rev.dispatch_phases dp ON dp.code = al.phase_code
    GROUP BY sd.stage_step_id, sd.dispatch_id
),
dispatched_steps AS (
                                                                                                                                                SELECT stage_step_id
    FROM step_dispatch_verdicts
    GROUP BY stage_step_id
    HAVING bool_or(verdict IS DISTINCT FROM 'failed')
        OR count(*) >= 3
),
fork_cutoff AS (
                                SELECT ps.ord AS fork_ord
    FROM rev.journeys j
    LEFT JOIN rev.playbook_stages ps ON ps.id = j.fork_stage_id
    WHERE j.id = $1::UUID
)
SELECT
    st.playbook_stage_id AS "playbook_stage_id!",
    st.stage_step_id AS "stage_step_id!",
    array_agg(st.step_task_id ORDER BY st.task_ord) AS "step_task_ids!"
FROM step_tasks st
WHERE st.stage_step_id IN (SELECT stage_step_id FROM computed_steps)
  AND st.stage_step_id NOT IN (SELECT stage_step_id FROM operator_steps)
  AND st.stage_step_id NOT IN (SELECT stage_step_id FROM dispatched_steps)
  AND (
      (SELECT fork_ord FROM fork_cutoff) IS NULL
      OR st.stage_ord >= (SELECT fork_ord FROM fork_cutoff)
  )
GROUP BY st.playbook_stage_id, st.stage_step_id, st.stage_ord, st.step_ord
ORDER BY st.stage_ord, st.step_ord;
