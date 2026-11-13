SELECT
    bu.brief_id,
    bu.id AS unit_id,
    bu.unit_key,
            bun.level AS "notice_level?",
    bun.title_text AS "notice_title_text?",
    bun.detail_text AS "notice_detail_text?",
        bum.markdown_text AS "markdown_text?",
            bum.deliverable_spec AS "markdown_deliverable_spec?",
        bict.key AS "input_control_type_key?",
    bui.label_text AS "input_label_text?",
    bui.help_text AS "input_help_text?",
    bui.placeholder_text AS "input_placeholder_text?",
    CASE WHEN bui.control_spec ? 'value_slot_id'
         THEN (bui.control_spec ->> 'value_slot_id')::UUID
         ELSE NULL
    END AS "input_slot_id?",
    vspd_in.name AS "input_slot_name?",
    vs_in.title AS "input_slot_title?",
    vk_in.key AS "input_slot_value_kind?",
            vs_in.cardinality AS "input_slot_cardinality?",
    vs_in.min_selections AS "input_slot_min_selections?",
    vs_in.max_selections AS "input_slot_max_selections?",
        CASE WHEN bui.control_spec ? 'candidate_source_value_slot_id'
         THEN (bui.control_spec ->> 'candidate_source_value_slot_id')::UUID
         ELSE NULL
    END AS "input_candidate_source_value_slot_id?",
    cs_vspd_in.name AS "input_candidate_source_name?",
            (bui.control_spec ->> 'optional')::boolean AS "input_optional?"
FROM rev.brief_units bu
LEFT JOIN rev.brief_unit_notice bun ON bun.brief_unit_id = bu.id
LEFT JOIN rev.brief_unit_markdown bum ON bum.brief_unit_id = bu.id
LEFT JOIN rev.brief_unit_input bui ON bui.brief_unit_id = bu.id
LEFT JOIN rev.brief_input_control_types bict ON bict.id = bui.control_type_id
LEFT JOIN rev.value_slots vs_in
    ON bui.control_spec ? 'value_slot_id'
   AND vs_in.id = (bui.control_spec ->> 'value_slot_id')::UUID
LEFT JOIN rev.versioned_objects vo_in ON vo_in.id = vs_in.versioned_object_id
LEFT JOIN rev.value_slot_path_details vspd_in ON vspd_in.path_id = vo_in.path_id
LEFT JOIN rev.value_kinds vk_in ON vk_in.id = vs_in.value_kind_id
LEFT JOIN rev.value_slots cs_vs_in
    ON bui.control_spec ? 'candidate_source_value_slot_id'
   AND cs_vs_in.id = (bui.control_spec ->> 'candidate_source_value_slot_id')::UUID
LEFT JOIN rev.versioned_objects cs_vo_in ON cs_vo_in.id = cs_vs_in.versioned_object_id
LEFT JOIN rev.value_slot_path_details cs_vspd_in ON cs_vspd_in.path_id = cs_vo_in.path_id
WHERE bu.brief_id = ANY($1::UUID[])
ORDER BY bu.brief_id, bu.ord, bu.id;
