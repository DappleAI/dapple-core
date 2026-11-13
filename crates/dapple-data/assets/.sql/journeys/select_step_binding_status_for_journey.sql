
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
operator_slot AS (
                        SELECT DISTINCT ON (vo.path_id)
        vo.path_id AS value_slot_path_id,
        vs.id AS value_slot_id,
        bu.id AS brief_unit_id,
                                        bu.ord AS authored_ord,
        bict.key AS control_type,
        bui.label_text,
        bui.help_text,
        bui.placeholder_text,
                                (bui.control_spec ->> 'candidate_source_value_slot_id')::UUID
            AS candidate_source_value_slot_id,
                        (bui.control_spec ->> 'optional')::boolean AS optional,
                                bui.control_spec -> 'item_fields' AS item_fields,
                                (bui.control_spec ->> 'refine_input_value_slot_id')::UUID
            AS refine_input_value_slot_id,
                                bui.control_spec -> 'axis' AS axis
    FROM journey_packs jp
    JOIN rev.brief_pack_groups bpg ON bpg.brief_pack_id = jp.brief_pack_id
    JOIN rev.brief_groups bg ON bg.id = bpg.brief_group_id
    JOIN rev.brief_group_briefs bgb ON bgb.brief_group_id = bg.id
    JOIN rev.briefs b ON b.id = bgb.brief_id
    JOIN rev.brief_units bu ON bu.brief_id = b.id
    JOIN rev.brief_unit_input bui ON bui.brief_unit_id = bu.id
    JOIN rev.brief_input_control_types bict ON bict.id = bui.control_type_id
    JOIN rev.value_slots vs
        ON bui.control_spec ? 'value_slot_id'
       AND vs.id = (bui.control_spec ->> 'value_slot_id')::UUID
    JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
    ORDER BY vo.path_id, vs.id DESC
),
computed_slot AS (
                        SELECT DISTINCT ON (vo.path_id)
        vo.path_id AS value_slot_path_id,
        vs.id AS value_slot_id,
                                dh.key AS display_kind,
        bo.reveal AS display_reveal,
        bo.title_text AS display_title,
                                        bo.display_group,
        bo.display_format,
                bo.ord AS authored_ord
    FROM journey_packs jp
    JOIN rev.brief_pack_groups bpg ON bpg.brief_pack_id = jp.brief_pack_id
    JOIN rev.brief_groups bg ON bg.id = bpg.brief_group_id
    JOIN rev.brief_group_briefs bgb ON bgb.brief_group_id = bg.id
    JOIN rev.briefs b ON b.id = bgb.brief_id
    JOIN rev.brief_outputs bo ON bo.brief_id = b.id
    JOIN rev.brief_output_source_kinds bosk
        ON bosk.id = bo.source_kind_id AND bosk.key = 'computed'
    JOIN rev.value_slots vs ON vs.id = bo.value_slot_id
    JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
    LEFT JOIN rev.brief_output_display_hints dh ON dh.id = bo.display_hint_id
    ORDER BY vo.path_id, vs.id DESC
),
journey_steps AS (
                                SELECT ps.id AS playbook_stage_id, ss.id AS stage_step_id, ss.step_id,
           ps.ord AS stage_ord, ss.ord AS step_ord,
           spd.name AS stage_name,
           COALESCE(rev.fluent_body(s.description_fluent_message_id, 'en'), s.description, spd.name) AS stage_description,
           sk.key AS stage_kind,
           s.icons AS stage_icon
    FROM rev.journeys j
    JOIN rev.playbook_stages ps ON ps.playbook_id = j.playbook_id
    JOIN rev.stage_steps ss ON ss.stage_id = ps.stage_id
    JOIN rev.stages s ON s.id = ps.stage_id
    JOIN rev.versioned_objects vo_s ON vo_s.id = s.versioned_object_id
    JOIN rev.stage_path_details spd ON spd.path_id = vo_s.path_id
    LEFT JOIN rev.stage_kinds sk ON sk.id = s.kind_id
    WHERE j.id = $1::UUID
),
step_tasks AS (
    SELECT
        jst.playbook_stage_id,
        jst.stage_step_id,
        stk.id AS step_task_id
    FROM journey_steps jst
    JOIN rev.step_tasks stk ON stk.step_id = jst.step_id
),
all_overlays AS (
    SELECT st.stage_step_id, st.step_task_id, o.value_slot_path_id, o.visibility, 1 AS priority
    FROM rev.playbook_stage_overlays o
    JOIN step_tasks st ON st.playbook_stage_id = o.playbook_stage_id
    UNION ALL
    SELECT st.stage_step_id, st.step_task_id, o.value_slot_path_id, o.visibility, 2 AS priority
    FROM rev.stage_step_overlays o
    JOIN step_tasks st ON st.stage_step_id = o.stage_step_id
    UNION ALL
    SELECT st.stage_step_id, st.step_task_id, o.value_slot_path_id, o.visibility, 3 AS priority
    FROM rev.step_task_overlays o
    JOIN step_tasks st ON st.step_task_id = o.step_task_id
),
ranked AS (
    SELECT DISTINCT ON (stage_step_id, step_task_id, value_slot_path_id)
        stage_step_id, value_slot_path_id, visibility
    FROM all_overlays
    ORDER BY stage_step_id, step_task_id, value_slot_path_id, priority DESC
),
required AS (
    SELECT DISTINCT stage_step_id, value_slot_path_id
    FROM ranked
    WHERE visibility = 'expose'
)
SELECT
    js.stage_step_id AS "stage_step_id!",
    js.playbook_stage_id AS "playbook_stage_id!",
    js.stage_name AS "stage_name!",
    js.stage_description AS "stage_description!",
    js.stage_kind AS "stage_kind?",
    js.stage_icon AS "stage_icon?",
    COALESCE(os.value_slot_id, cs.value_slot_id) AS "value_slot_id?",
    vspd.name AS "value_slot_name?",
    vk.key AS "value_kind?",
    (CASE
        WHEN req.value_slot_path_id IS NULL THEN NULL
        WHEN os.value_slot_path_id IS NOT NULL THEN 'operator'
        WHEN cs.value_slot_path_id IS NOT NULL THEN 'computed'
                                ELSE 'operator'
    END)::TEXT AS "producer?",
    os.control_type AS "control_type?",
    os.label_text AS "label_text?",
    os.help_text AS "help_text?",
    os.placeholder_text AS "placeholder_text?",
    os.brief_unit_id AS "brief_unit_id?",
                    rvs.cardinality AS "cardinality?",
    rvs.min_selections AS "min_selections?",
    rvs.max_selections AS "max_selections?",
            os.candidate_source_value_slot_id AS "candidate_source_value_slot_id?",
    cs_vspd.name AS "candidate_source_name?",
            os.optional AS "optional?",
        os.item_fields AS "item_fields?",
    os.refine_input_value_slot_id AS "refine_input_value_slot_id?",
        os.axis AS "axis?",
                cs.display_kind AS "display_kind?",
    cs.display_reveal AS "display_reveal?",
    cs.display_title AS "display_title?",
    cs.display_group AS "display_group?",
    cs.display_format AS "display_format?",
    (
        req.value_slot_path_id IS NOT NULL
        AND EXISTS (
            SELECT 1
            FROM rev.value_slots vs2
            JOIN rev.versioned_objects vo2 ON vo2.id = vs2.versioned_object_id
            JOIN rev.dispatch_intent_bindings dib ON dib.value_slot_id = vs2.id
            JOIN rev.dispatches d ON d.id = dib.dispatch_id
            JOIN journey_chain jc ON jc.journey_id = d.journey_id
            WHERE vo2.path_id = req.value_slot_path_id
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
    ) AS "bound?",
                                                                (
        os.candidate_source_value_slot_id IS NOT NULL
        AND NOT COALESCE(os.optional, false)
        AND COALESCE(rvs.min_selections, 1) > 0
        AND COALESCE((
            SELECT dib.produced_empty
            FROM rev.value_slots cvs
            JOIN rev.versioned_objects cvo ON cvo.id = cvs.versioned_object_id
            JOIN rev.dispatch_intent_bindings dib ON dib.value_slot_id = cvs.id
            JOIN rev.dispatches d ON d.id = dib.dispatch_id
            JOIN journey_chain jc ON jc.journey_id = d.journey_id
            WHERE cvo.path_id = cs_vo.path_id
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
    ) AS "blocked?"
FROM journey_steps js
LEFT JOIN required req ON req.stage_step_id = js.stage_step_id
LEFT JOIN operator_slot os ON os.value_slot_path_id = req.value_slot_path_id
LEFT JOIN computed_slot cs ON cs.value_slot_path_id = req.value_slot_path_id
LEFT JOIN rev.value_slot_path_details vspd ON vspd.path_id = req.value_slot_path_id
LEFT JOIN rev.value_slots rvs ON rvs.id = COALESCE(os.value_slot_id, cs.value_slot_id)
LEFT JOIN rev.value_kinds vk ON vk.id = COALESCE(rvs.value_kind_id, vspd.value_kind_id)
LEFT JOIN rev.value_slots cs_vs ON cs_vs.id = os.candidate_source_value_slot_id
LEFT JOIN rev.versioned_objects cs_vo ON cs_vo.id = cs_vs.versioned_object_id
LEFT JOIN rev.value_slot_path_details cs_vspd ON cs_vspd.path_id = cs_vo.path_id
ORDER BY js.stage_ord,
         js.step_ord,
         COALESCE(os.authored_ord, cs.authored_ord),
         vspd.name,
         COALESCE(os.value_slot_id, cs.value_slot_id);
