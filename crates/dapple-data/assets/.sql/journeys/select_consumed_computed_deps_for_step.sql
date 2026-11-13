
WITH RECURSIVE
journey_chain(journey_id, depth, fork_cutoff_ord) AS (
    SELECT $3::UUID, 0, NULL::INTEGER
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
    SELECT $3::UUID AS journey_id
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
computed_output_paths AS (
        SELECT DISTINCT vo.path_id AS value_slot_path_id
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
),
playbook_context AS (
            SELECT ps.playbook_id
    FROM rev.playbook_stages ps
    JOIN rev.stage_steps ss
        ON ss.id = $2::UUID AND ss.stage_id = ps.stage_id
    WHERE ps.id = $1::UUID
),
playbook_step_tasks AS (
        SELECT
        stk.id AS step_task_id,
        ss.id AS stage_step_id,
        ps.id AS playbook_stage_id,
        ps.ord AS stage_ord,
        ss.ord AS step_ord
    FROM playbook_context pc
    JOIN rev.playbook_stages ps ON ps.playbook_id = pc.playbook_id
    JOIN rev.stage_steps ss ON ss.stage_id = ps.stage_id
    JOIN rev.step_tasks stk ON stk.step_id = ss.step_id
),
playbook_overlays AS (
    SELECT pst.step_task_id, pst.stage_step_id, pst.playbook_stage_id,
           pst.stage_ord, pst.step_ord, o.value_slot_path_id, o.visibility, 1 AS priority
    FROM rev.playbook_stage_overlays o
    JOIN playbook_step_tasks pst ON pst.playbook_stage_id = o.playbook_stage_id
    UNION ALL
    SELECT pst.step_task_id, pst.stage_step_id, pst.playbook_stage_id,
           pst.stage_ord, pst.step_ord, o.value_slot_path_id, o.visibility, 2 AS priority
    FROM rev.stage_step_overlays o
    JOIN playbook_step_tasks pst ON pst.stage_step_id = o.stage_step_id
    UNION ALL
    SELECT pst.step_task_id, pst.stage_step_id, pst.playbook_stage_id,
           pst.stage_ord, pst.step_ord, o.value_slot_path_id, o.visibility, 3 AS priority
    FROM rev.step_task_overlays o
    JOIN playbook_step_tasks pst ON pst.step_task_id = o.step_task_id
),
playbook_ranked AS (
            SELECT DISTINCT ON (step_task_id, value_slot_path_id)
        stage_step_id, playbook_stage_id, stage_ord, step_ord, value_slot_path_id, visibility
    FROM playbook_overlays
    ORDER BY step_task_id, value_slot_path_id, priority DESC
),
playbook_exposed_computed AS (
        SELECT DISTINCT
        pr.stage_step_id, pr.playbook_stage_id, pr.stage_ord, pr.step_ord, pr.value_slot_path_id
    FROM playbook_ranked pr
    JOIN computed_output_paths cop ON cop.value_slot_path_id = pr.value_slot_path_id
    WHERE pr.visibility = 'expose'
),
target_deps AS (
        SELECT t.value_slot_path_id
    FROM playbook_exposed_computed t
    WHERE t.stage_step_id = $2::UUID
      AND t.playbook_stage_id = $1::UUID
      AND EXISTS (
          SELECT 1
          FROM playbook_exposed_computed e
          WHERE e.value_slot_path_id = t.value_slot_path_id
            AND e.stage_ord < t.stage_ord
      )
),
producers AS (
        SELECT DISTINCT ON (e.value_slot_path_id)
        e.value_slot_path_id,
        e.playbook_stage_id AS producer_playbook_stage_id,
        e.stage_step_id AS producer_stage_step_id
    FROM playbook_exposed_computed e
    JOIN target_deps td ON td.value_slot_path_id = e.value_slot_path_id
    ORDER BY e.value_slot_path_id, e.stage_ord, e.step_ord
)
SELECT
    td.value_slot_path_id AS "value_slot_path_id!",
    p.producer_playbook_stage_id AS "producer_playbook_stage_id!",
    p.producer_stage_step_id AS "producer_stage_step_id!",
    EXISTS (
        SELECT 1
        FROM rev.value_slots vs
        JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
        JOIN rev.dispatch_intent_bindings dib ON dib.value_slot_id = vs.id
        JOIN rev.dispatches d ON d.id = dib.dispatch_id
        JOIN journey_chain jc ON jc.journey_id = d.journey_id
        WHERE vo.path_id = td.value_slot_path_id
          AND (
    jc.fork_cutoff_ord IS NULL
    OR COALESCE((
        SELECT MAX(fps.ord)
        FROM rev.dispatch_stages fds
        JOIN rev.playbook_stages fps ON fps.id = fds.playbook_stage_id
        WHERE fds.dispatch_id = d.id
    ), 2147483647) < jc.fork_cutoff_ord
)

    ) AS "bound!"
FROM target_deps td
JOIN producers p ON p.value_slot_path_id = td.value_slot_path_id;
