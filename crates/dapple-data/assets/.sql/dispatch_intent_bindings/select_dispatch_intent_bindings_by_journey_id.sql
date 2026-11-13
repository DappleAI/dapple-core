
SELECT
    dib.id,
    dib.dispatch_id,
    dib.value_slot_id,
    vspd.name as value_slot_name,
    vk.key as value_slot_value_kind,
    dib.created_at,
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
FROM rev.dispatches d
JOIN rev.dispatch_intent_bindings dib ON dib.dispatch_id = d.id
JOIN rev.value_slots vs ON vs.id = dib.value_slot_id
JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.value_slot_path_details vspd ON vspd.path_id = op.id
JOIN rev.value_kinds vk ON vk.id = vs.value_kind_id
LEFT JOIN rev.dispatch_intent_binding_text_items ti ON ti.dispatch_intent_binding_id = dib.id
LEFT JOIN rev.dispatch_intent_binding_int_items ii ON ii.dispatch_intent_binding_id = dib.id
LEFT JOIN rev.dispatch_intent_binding_number_items ni ON ni.dispatch_intent_binding_id = dib.id
LEFT JOIN rev.dispatch_intent_binding_bool_items bi ON bi.dispatch_intent_binding_id = dib.id
LEFT JOIN rev.dispatch_intent_binding_json_items ji ON ji.dispatch_intent_binding_id = dib.id
LEFT JOIN rev.dispatch_intent_binding_asset_items ai ON ai.dispatch_intent_binding_id = dib.id
LEFT JOIN rev.assets a ON a.id = ai.asset_id
LEFT JOIN rev.dispatch_intent_binding_value_meta vm ON vm.dispatch_intent_binding_id = dib.id
LEFT JOIN rev.dispatch_intent_binding_dataset_items dsi ON dsi.dispatch_intent_binding_id = dib.id
LEFT JOIN rev.asset_dataset_extractions ade ON ade.id = dsi.asset_dataset_extraction_id
WHERE d.journey_id = $1::UUID
ORDER BY dib.created_at ASC, dib.id ASC, ti.id, ii.id, ni.id, bi.id, ji.id, ai.id, vm.id, dsi.id;
