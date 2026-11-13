
WITH RECURSIVE
journey_chain(journey_id, depth, fork_cutoff_ord) AS (
    SELECT $2::UUID, 0, NULL::INTEGER
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
    SELECT $2::UUID AS journey_id
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
            AND COALESCE(vs.min_selections, 1) > 0) AS required
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
    ORDER BY vo.path_id, vs.id DESC
),
stage_tasks AS (
    SELECT
        stk.id AS step_task_id,
        ss.id AS stage_step_id,
        pstage.id AS playbook_stage_id
    FROM rev.playbook_stages pstage
    JOIN rev.stage_steps ss ON ss.stage_id = pstage.stage_id
    JOIN rev.step_tasks stk ON stk.step_id = ss.step_id
    WHERE pstage.id = $1::UUID
),
all_overlays AS (
    SELECT st.step_task_id, o.value_slot_path_id, o.visibility, 1 AS priority
    FROM rev.playbook_stage_overlays o
    JOIN stage_tasks st ON st.playbook_stage_id = o.playbook_stage_id
    UNION ALL
    SELECT st.step_task_id, o.value_slot_path_id, o.visibility, 2 AS priority
    FROM rev.stage_step_overlays o
    JOIN stage_tasks st ON st.stage_step_id = o.stage_step_id
    UNION ALL
    SELECT st.step_task_id, o.value_slot_path_id, o.visibility, 3 AS priority
    FROM rev.step_task_overlays o
    JOIN stage_tasks st ON st.step_task_id = o.step_task_id
),
ranked AS (
    SELECT DISTINCT ON (step_task_id, value_slot_path_id)
        step_task_id, value_slot_path_id, visibility
    FROM all_overlays
    ORDER BY step_task_id, value_slot_path_id, priority DESC
),
required_paths AS (
                            SELECT DISTINCT r.value_slot_path_id
    FROM ranked r
    LEFT JOIN journey_operator_inputs joi
        ON joi.value_slot_path_id = r.value_slot_path_id
    WHERE r.visibility = 'expose'
      AND COALESCE(joi.required, true)
)
SELECT NOT EXISTS (
    SELECT 1
    FROM required_paths rp
    WHERE NOT EXISTS (
        SELECT 1
        FROM rev.value_slots vs
        JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
        JOIN rev.dispatch_intent_bindings dib ON dib.value_slot_id = vs.id
        JOIN rev.dispatches d ON d.id = dib.dispatch_id
        JOIN journey_chain jc ON jc.journey_id = d.journey_id
        WHERE vo.path_id = rp.value_slot_path_id
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
) AS fully_bound;
