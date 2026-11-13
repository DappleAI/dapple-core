WITH RECURSIVE
target_journey AS (
    SELECT journey_id FROM rev.dispatches WHERE id = $1::UUID
),
journey_chain(journey_id, depth, fork_cutoff_ord) AS (
    SELECT (SELECT journey_id FROM target_journey), 0, NULL::INTEGER
    UNION ALL
    SELECT j.predecessor_journey_id, jc.depth + 1, ps.ord
    FROM journey_chain jc
    JOIN rev.journeys j ON j.id = jc.journey_id
    LEFT JOIN rev.playbook_stages ps ON ps.id = j.fork_stage_id
    WHERE j.predecessor_journey_id IS NOT NULL
      AND jc.depth < 50
),
chain_bindings AS (
    SELECT
        dib.id AS binding_id,
        dib.value_slot_id,
        dib.created_at AS bound_at,
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
journey_bindings AS (
    SELECT DISTINCT ON (cb.value_slot_id)
        cb.binding_id,
        cb.value_slot_id
    FROM chain_bindings cb
    ORDER BY cb.value_slot_id, cb.depth ASC, cb.bound_at DESC, cb.binding_id DESC
)
SELECT
    jb.value_slot_id,
            jb.binding_id AS "binding_id!",
                vo.path_id AS "value_slot_path_id!",
    vspd.name AS value_slot_name,
    vk.key AS value_kind,
    vs.cardinality AS "cardinality!",
    (SELECT ti.value FROM rev.dispatch_intent_binding_text_items ti
        WHERE ti.dispatch_intent_binding_id = jb.binding_id
        ORDER BY ti.id LIMIT 1) AS text_value,
    (SELECT ii.value FROM rev.dispatch_intent_binding_int_items ii
        WHERE ii.dispatch_intent_binding_id = jb.binding_id
        ORDER BY ii.id LIMIT 1) AS int_value,
    (SELECT ni.value FROM rev.dispatch_intent_binding_number_items ni
        WHERE ni.dispatch_intent_binding_id = jb.binding_id
        ORDER BY ni.id LIMIT 1) AS number_value,
    (SELECT bi.value FROM rev.dispatch_intent_binding_bool_items bi
        WHERE bi.dispatch_intent_binding_id = jb.binding_id
        ORDER BY bi.id LIMIT 1) AS bool_value,
                    (CASE WHEN vs.constraints ->> 'delivery' = 'envelope' THEN NULL
     ELSE (SELECT ji.value FROM rev.dispatch_intent_binding_json_items ji
        WHERE ji.dispatch_intent_binding_id = jb.binding_id
        ORDER BY ji.id LIMIT 1)
    END) AS json_value,
    (SELECT ai.asset_id FROM rev.dispatch_intent_binding_asset_items ai
        WHERE ai.dispatch_intent_binding_id = jb.binding_id
        ORDER BY ai.id LIMIT 1) AS asset_id,
            (SELECT ade.envelope FROM rev.dispatch_intent_binding_dataset_items dsi
        JOIN rev.asset_dataset_extractions ade ON ade.id = dsi.asset_dataset_extraction_id
        WHERE dsi.dispatch_intent_binding_id = jb.binding_id
        ORDER BY dsi.id LIMIT 1) AS envelope,
                vs.constraints AS "value_slot_constraints!",
                                    (CASE WHEN vs.cardinality != 'multi'
            OR vs.constraints ->> 'delivery' = 'envelope' THEN '[]'::jsonb
     ELSE CASE vk.key
        WHEN 'text' THEN COALESCE((SELECT jsonb_agg(x.value ORDER BY x.id)
            FROM rev.dispatch_intent_binding_text_items x
            WHERE x.dispatch_intent_binding_id = jb.binding_id), '[]'::jsonb)
        WHEN 'int' THEN COALESCE((SELECT jsonb_agg(x.value ORDER BY x.id)
            FROM rev.dispatch_intent_binding_int_items x
            WHERE x.dispatch_intent_binding_id = jb.binding_id), '[]'::jsonb)
        WHEN 'number' THEN COALESCE((SELECT jsonb_agg(x.value ORDER BY x.id)
            FROM rev.dispatch_intent_binding_number_items x
            WHERE x.dispatch_intent_binding_id = jb.binding_id), '[]'::jsonb)
        WHEN 'bool' THEN COALESCE((SELECT jsonb_agg(x.value ORDER BY x.id)
            FROM rev.dispatch_intent_binding_bool_items x
            WHERE x.dispatch_intent_binding_id = jb.binding_id), '[]'::jsonb)
        WHEN 'json' THEN COALESCE((SELECT jsonb_agg(x.value ORDER BY x.id)
            FROM rev.dispatch_intent_binding_json_items x
            WHERE x.dispatch_intent_binding_id = jb.binding_id), '[]'::jsonb)
        WHEN 'asset' THEN COALESCE((SELECT jsonb_agg(jsonb_build_object('asset_id', x.asset_id) ORDER BY x.id)
            FROM rev.dispatch_intent_binding_asset_items x
            WHERE x.dispatch_intent_binding_id = jb.binding_id), '[]'::jsonb)
        WHEN 'dataset' THEN COALESCE((SELECT jsonb_agg(ade.envelope ORDER BY x.id)
            FROM rev.dispatch_intent_binding_dataset_items x
            JOIN rev.asset_dataset_extractions ade ON ade.id = x.asset_dataset_extraction_id
            WHERE x.dispatch_intent_binding_id = jb.binding_id), '[]'::jsonb)
        ELSE '[]'::jsonb
     END
    END) AS "item_values!"
FROM journey_bindings jb
JOIN rev.value_slots vs ON vs.id = jb.value_slot_id
JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
JOIN rev.value_slot_path_details vspd ON vspd.path_id = vo.path_id
JOIN rev.value_kinds vk ON vk.id = vs.value_kind_id
ORDER BY jb.value_slot_id;
