
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
journey_scope AS (
    SELECT $1::UUID AS journey_id
),
journey_packs AS (
    SELECT DISTINCT ON (bpvo.path_id) bpack.id AS brief_pack_id
    FROM journey_scope js
    JOIN rev.journey_brief_packs jbp ON jbp.journey_id = js.journey_id
    JOIN rev.brief_packs bpack ON bpack.id = jbp.brief_pack_id
    JOIN rev.versioned_objects bpvo ON bpvo.id = bpack.versioned_object_id
    ORDER BY bpvo.path_id, bpvo.created_at DESC, bpvo.id DESC
)

,
journey_operator_inputs AS (
                        SELECT DISTINCT ON (vo.path_id)
        vo.path_id AS value_slot_path_id,
        (NOT COALESCE((bui.control_spec ->> 'optional')::boolean, false)
            AND COALESCE(vs.min_selections, 1) > 0) AS required,
                                        cs_vo.path_id AS candidate_source_path_id
    FROM journey_packs jp
    JOIN rev.brief_pack_groups bpg ON bpg.brief_pack_id = jp.brief_pack_id
    JOIN rev.brief_groups bg ON bg.id = bpg.brief_group_id
    JOIN rev.brief_group_briefs bgb ON bgb.brief_group_id = bg.id
    JOIN rev.briefs b ON b.id = bgb.brief_id
    JOIN rev.brief_units bu ON bu.brief_id = b.id
    JOIN rev.brief_unit_input bui ON bui.brief_unit_id = bu.id
    JOIN rev.value_slots vs
        ON bui.control_spec ? 'value_slot_id'
       AND vs.id = (bui.control_spec ->> 'value_slot_id')::UUID
    JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
    LEFT JOIN rev.value_slots cs_vs
        ON bui.control_spec ? 'candidate_source_value_slot_id'
       AND cs_vs.id = (bui.control_spec ->> 'candidate_source_value_slot_id')::UUID
    LEFT JOIN rev.versioned_objects cs_vo ON cs_vo.id = cs_vs.versioned_object_id
    ORDER BY vo.path_id, vs.id DESC
),
journey_stages AS (
                        SELECT ps.id AS playbook_stage_id, ps.ord AS stage_ord,
           spd.name,
           COALESCE(rev.fluent_body(s.description_fluent_message_id, 'en'), s.description, spd.name) AS description,
           sk.key AS kind,
           s.icons AS icon
    FROM rev.journeys j
    JOIN rev.playbook_stages ps ON ps.playbook_id = j.playbook_id
    JOIN rev.stages s ON s.id = ps.stage_id
    JOIN rev.versioned_objects vo_s ON vo_s.id = s.versioned_object_id
    JOIN rev.stage_path_details spd ON spd.path_id = vo_s.path_id
    LEFT JOIN rev.stage_kinds sk ON sk.id = s.kind_id
    WHERE j.id = $1::UUID
),
stage_tasks AS (
    SELECT
        js.playbook_stage_id,
        stk.id AS step_task_id,
        ss.id AS stage_step_id
    FROM journey_stages js
    JOIN rev.playbook_stages pstage ON pstage.id = js.playbook_stage_id
    JOIN rev.stage_steps ss ON ss.stage_id = pstage.stage_id
    JOIN rev.step_tasks stk ON stk.step_id = ss.step_id
),
all_overlays AS (
    SELECT st.playbook_stage_id, st.step_task_id, o.value_slot_path_id, o.visibility, 1 AS priority
    FROM rev.playbook_stage_overlays o
    JOIN stage_tasks st ON st.playbook_stage_id = o.playbook_stage_id
    UNION ALL
    SELECT st.playbook_stage_id, st.step_task_id, o.value_slot_path_id, o.visibility, 2 AS priority
    FROM rev.stage_step_overlays o
    JOIN stage_tasks st ON st.stage_step_id = o.stage_step_id
    UNION ALL
    SELECT st.playbook_stage_id, st.step_task_id, o.value_slot_path_id, o.visibility, 3 AS priority
    FROM rev.step_task_overlays o
    JOIN stage_tasks st ON st.step_task_id = o.step_task_id
),
ranked AS (
    SELECT DISTINCT ON (playbook_stage_id, step_task_id, value_slot_path_id)
        playbook_stage_id, value_slot_path_id, visibility
    FROM all_overlays
    ORDER BY playbook_stage_id, step_task_id, value_slot_path_id, priority DESC
),
required AS (
                    SELECT DISTINCT r.playbook_stage_id, r.value_slot_path_id
    FROM ranked r
    LEFT JOIN journey_operator_inputs joi
        ON joi.value_slot_path_id = r.value_slot_path_id
    WHERE r.visibility = 'expose'
      AND COALESCE(joi.required, true)
),
unbound AS (
    SELECT DISTINCT req.playbook_stage_id
    FROM required req
    WHERE NOT EXISTS (
        SELECT 1
        FROM rev.value_slots vs
        JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
        JOIN rev.dispatch_intent_bindings dib ON dib.value_slot_id = vs.id
        JOIN rev.dispatches d ON d.id = dib.dispatch_id
        JOIN journey_chain jc ON jc.journey_id = d.journey_id
        WHERE vo.path_id = req.value_slot_path_id
          AND (
    jc.fork_cutoff_ord IS NULL
    OR COALESCE((
        SELECT MAX(fps.ord)
        FROM rev.dispatch_stages fds
        JOIN rev.playbook_stages fps ON fps.id = fds.playbook_stage_id
        WHERE fds.dispatch_id = d.id
    ), 2147483647) < jc.fork_cutoff_ord
)

    )
),
blocked_stages AS (
                                            SELECT DISTINCT req.playbook_stage_id
    FROM required req
    JOIN journey_operator_inputs joi
        ON joi.value_slot_path_id = req.value_slot_path_id
    WHERE joi.candidate_source_path_id IS NOT NULL
      AND joi.required
      AND req.playbook_stage_id IN (SELECT playbook_stage_id FROM unbound)
      AND COALESCE((
          SELECT dib.produced_empty
          FROM rev.value_slots cvs
          JOIN rev.versioned_objects cvo ON cvo.id = cvs.versioned_object_id
          JOIN rev.dispatch_intent_bindings dib ON dib.value_slot_id = cvs.id
          JOIN rev.dispatches d ON d.id = dib.dispatch_id
          JOIN journey_chain jc ON jc.journey_id = d.journey_id
          WHERE cvo.path_id = joi.candidate_source_path_id
            AND (
    jc.fork_cutoff_ord IS NULL
    OR COALESCE((
        SELECT MAX(fps.ord)
        FROM rev.dispatch_stages fds
        JOIN rev.playbook_stages fps ON fps.id = fds.playbook_stage_id
        WHERE fds.dispatch_id = d.id
    ), 2147483647) < jc.fork_cutoff_ord
)

          ORDER BY dib.id DESC
          LIMIT 1
      ), false)
)
SELECT
    js.playbook_stage_id AS "playbook_stage_id!",
    js.name AS "name!",
    js.description AS "description!",
    js.kind AS "kind?",
    js.icon AS "icon?",
    (js.playbook_stage_id NOT IN (SELECT playbook_stage_id FROM unbound)) AS "done!",
    (js.playbook_stage_id IN (SELECT playbook_stage_id FROM blocked_stages)) AS "blocked!"
FROM journey_stages js
ORDER BY js.stage_ord;
