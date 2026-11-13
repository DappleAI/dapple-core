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
chain_bindings AS (
    SELECT
        dib.id,
        dib.dispatch_id,
        dib.value_slot_id,
        dib.created_at,
        jc.depth
    FROM journey_chain jc
    JOIN rev.dispatches d ON d.journey_id = jc.journey_id
    JOIN rev.dispatch_intent_bindings dib ON dib.dispatch_id = d.id
    WHERE (
    jc.fork_cutoff_ord IS NULL
    OR COALESCE((
        SELECT MAX(fps.ord)
        FROM rev.dispatch_stages fds
        JOIN rev.playbook_stages fps ON fps.id = fds.playbook_stage_id
        WHERE fds.dispatch_id = d.id
    ), 2147483647) < jc.fork_cutoff_ord
)

),
ranked AS (
    SELECT
        cb.*,
        ROW_NUMBER() OVER (
            PARTITION BY cb.value_slot_id
            ORDER BY cb.depth ASC, cb.created_at DESC, cb.id DESC
        ) AS rnk
    FROM chain_bindings cb
)
SELECT
    r.id,
    r.dispatch_id,
    r.value_slot_id,
    vspd.name as value_slot_name,
    vk.key as value_slot_value_kind,
    r.created_at,
        ti.id as "text_id?",
    ti.value as "text_value?",
        ii.id as "int_id?",
    ii.value as "int_value?",
        ni.id as "number_id?",
    ni.value as "number_value?",
        bi.id as "bool_id?",
    bi.value as "bool_value?",
        ji.id as "json_id?",
    ji.value as "json_value?",
        ai.id as "asset_item_id?",
    ai.asset_id as "asset_id?",
        vm.id as "value_meta_id?",
    vm.ui_spec as "value_meta_ui_spec?",
    vm.producer as "value_meta_producer?",
                    dsi.id as "dataset_item_id?",
    dsi.asset_dataset_extraction_id as "dataset_extraction_id?",
    ade.envelope as "dataset_envelope?",
    vs.constraints as "value_slot_constraints!"
FROM ranked r
JOIN rev.value_slots vs ON vs.id = r.value_slot_id
JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.value_slot_path_details vspd ON vspd.path_id = op.id
JOIN rev.value_kinds vk ON vk.id = vs.value_kind_id
LEFT JOIN rev.dispatch_intent_binding_text_items ti ON ti.dispatch_intent_binding_id = r.id
LEFT JOIN rev.dispatch_intent_binding_int_items ii ON ii.dispatch_intent_binding_id = r.id
LEFT JOIN rev.dispatch_intent_binding_number_items ni ON ni.dispatch_intent_binding_id = r.id
LEFT JOIN rev.dispatch_intent_binding_bool_items bi ON bi.dispatch_intent_binding_id = r.id
LEFT JOIN rev.dispatch_intent_binding_json_items ji ON ji.dispatch_intent_binding_id = r.id
LEFT JOIN rev.dispatch_intent_binding_asset_items ai ON ai.dispatch_intent_binding_id = r.id
LEFT JOIN rev.dispatch_intent_binding_value_meta vm ON vm.dispatch_intent_binding_id = r.id
LEFT JOIN rev.dispatch_intent_binding_dataset_items dsi ON dsi.dispatch_intent_binding_id = r.id
LEFT JOIN rev.asset_dataset_extractions ade ON ade.id = dsi.asset_dataset_extraction_id
WHERE r.rnk = 1
ORDER BY r.created_at ASC, r.id ASC,
         ti.id, ii.id, ni.id, bi.id, ji.id, ai.id, vm.id, dsi.id;
