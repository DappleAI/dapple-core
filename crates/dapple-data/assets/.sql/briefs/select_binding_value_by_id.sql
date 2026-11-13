SELECT
    dib.value_slot_id,
    vk.key AS value_kind,
    vs.cardinality AS "cardinality!",
    (SELECT ti.value FROM rev.dispatch_intent_binding_text_items ti
        WHERE ti.dispatch_intent_binding_id = dib.id
        ORDER BY ti.id LIMIT 1) AS text_value,
    (SELECT ii.value FROM rev.dispatch_intent_binding_int_items ii
        WHERE ii.dispatch_intent_binding_id = dib.id
        ORDER BY ii.id LIMIT 1) AS int_value,
    (SELECT ni.value FROM rev.dispatch_intent_binding_number_items ni
        WHERE ni.dispatch_intent_binding_id = dib.id
        ORDER BY ni.id LIMIT 1) AS number_value,
    (SELECT bi.value FROM rev.dispatch_intent_binding_bool_items bi
        WHERE bi.dispatch_intent_binding_id = dib.id
        ORDER BY bi.id LIMIT 1) AS bool_value,
    (SELECT ji.value FROM rev.dispatch_intent_binding_json_items ji
        WHERE ji.dispatch_intent_binding_id = dib.id
        ORDER BY ji.id LIMIT 1) AS json_value,
    (SELECT ai.asset_id FROM rev.dispatch_intent_binding_asset_items ai
        WHERE ai.dispatch_intent_binding_id = dib.id
        ORDER BY ai.id LIMIT 1) AS asset_id,
    (SELECT ade.envelope FROM rev.dispatch_intent_binding_dataset_items dsi
        JOIN rev.asset_dataset_extractions ade ON ade.id = dsi.asset_dataset_extraction_id
        WHERE dsi.dispatch_intent_binding_id = dib.id
        ORDER BY dsi.id LIMIT 1) AS envelope,
    vs.constraints AS "value_slot_constraints!",
    (CASE vk.key
        WHEN 'text' THEN COALESCE((SELECT jsonb_agg(x.value ORDER BY x.id)
            FROM rev.dispatch_intent_binding_text_items x
            WHERE x.dispatch_intent_binding_id = dib.id), '[]'::jsonb)
        WHEN 'int' THEN COALESCE((SELECT jsonb_agg(x.value ORDER BY x.id)
            FROM rev.dispatch_intent_binding_int_items x
            WHERE x.dispatch_intent_binding_id = dib.id), '[]'::jsonb)
        WHEN 'number' THEN COALESCE((SELECT jsonb_agg(x.value ORDER BY x.id)
            FROM rev.dispatch_intent_binding_number_items x
            WHERE x.dispatch_intent_binding_id = dib.id), '[]'::jsonb)
        WHEN 'bool' THEN COALESCE((SELECT jsonb_agg(x.value ORDER BY x.id)
            FROM rev.dispatch_intent_binding_bool_items x
            WHERE x.dispatch_intent_binding_id = dib.id), '[]'::jsonb)
        WHEN 'json' THEN COALESCE((SELECT jsonb_agg(x.value ORDER BY x.id)
            FROM rev.dispatch_intent_binding_json_items x
            WHERE x.dispatch_intent_binding_id = dib.id), '[]'::jsonb)
        WHEN 'asset' THEN COALESCE((SELECT jsonb_agg(jsonb_build_object('asset_id', x.asset_id) ORDER BY x.id)
            FROM rev.dispatch_intent_binding_asset_items x
            WHERE x.dispatch_intent_binding_id = dib.id), '[]'::jsonb)
        WHEN 'dataset' THEN COALESCE((SELECT jsonb_agg(ade.envelope ORDER BY x.id)
            FROM rev.dispatch_intent_binding_dataset_items x
            JOIN rev.asset_dataset_extractions ade ON ade.id = x.asset_dataset_extraction_id
            WHERE x.dispatch_intent_binding_id = dib.id), '[]'::jsonb)
        ELSE '[]'::jsonb
    END) AS "item_values!"
FROM rev.dispatch_intent_bindings dib
JOIN rev.value_slots vs ON vs.id = dib.value_slot_id
JOIN rev.value_kinds vk ON vk.id = vs.value_kind_id
WHERE dib.id = $1::UUID;
