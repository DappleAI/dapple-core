
WITH
journey_scope AS (
        SELECT d.journey_id
    FROM rev.dispatches d
    WHERE d.id = $1::UUID
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
computed_outputs AS (
                    SELECT
        bo.value_slot_id,
        vo.path_id AS value_slot_path_id,
        bo.source_path,
        vspd.name AS value_slot_name,
        vk.key AS value_kind_key,
        vs.cardinality
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
    JOIN rev.value_slot_path_details vspd ON vspd.path_id = vo.path_id
    JOIN rev.value_kinds vk ON vk.id = vs.value_kind_id
),
stage_tasks AS (
    SELECT
        stk.id AS step_task_id,
        ss.id AS stage_step_id,
        pstage.id AS playbook_stage_id
    FROM rev.playbook_stages pstage
    JOIN rev.stage_steps ss ON ss.stage_id = pstage.stage_id
    JOIN rev.step_tasks stk ON stk.step_id = ss.step_id
    WHERE pstage.id = $2::UUID
      AND ss.id = $3::UUID
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
exposed_paths AS (
    SELECT DISTINCT value_slot_path_id
    FROM ranked
    WHERE visibility = 'expose'
)
SELECT DISTINCT
    co.value_slot_id AS "value_slot_id!",
    co.source_path AS "source_path!",
    co.value_slot_name AS "value_slot_name!",
    co.value_kind_key AS "value_kind_key!",
    co.cardinality AS "cardinality!"
FROM computed_outputs co
JOIN exposed_paths ep ON ep.value_slot_path_id = co.value_slot_path_id
ORDER BY co.value_slot_id, co.source_path;
