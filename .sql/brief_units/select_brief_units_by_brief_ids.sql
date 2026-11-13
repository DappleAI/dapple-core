SELECT
    bu.id, b.id AS brief_id, bu.unit_key,
    bu.layout, bu.requires, bu.extensions, bu.created_at,
    -- markdown (1:1)
    bum.markdown_text, bum.fluent_message_id AS "markdown_fluent_message_id?",
    bum.format AS markdown_format, bum.deliverable_spec AS "markdown_deliverable_spec?",
    -- notice (1:1)
    bun.level AS "notice_level?",
    bun.title_text AS notice_title_text,
    bun.title_fluent_message_id AS "notice_title_fluent_message_id?",
    bun.detail_text AS notice_detail_text,
    bun.detail_fluent_message_id AS "notice_detail_fluent_message_id?",
    -- input (1:1)
    bui.control_type_id AS "input_control_type_id?",
    ct.key AS "input_control_type_key?",
    bui.label_text AS input_label_text,
    bui.label_fluent_message_id AS "input_label_fluent_message_id?",
    bui.help_text AS input_help_text,
    bui.help_fluent_message_id AS "input_help_fluent_message_id?",
    bui.placeholder_text AS input_placeholder_text,
    bui.placeholder_fluent_message_id AS "input_placeholder_fluent_message_id?",
    bui.control_spec AS "input_control_spec?",
    -- media marker (1:1)
    bumed.created_at AS "media_created_at?"
FROM rev.brief_units bu
JOIN rev.briefs b ON b.id = bu.brief_id
LEFT JOIN rev.brief_unit_markdown bum ON bum.brief_unit_id = bu.id
LEFT JOIN rev.brief_unit_notice bun ON bun.brief_unit_id = bu.id
LEFT JOIN rev.brief_unit_input bui ON bui.brief_unit_id = bu.id
LEFT JOIN rev.brief_input_control_types ct ON ct.id = bui.control_type_id
LEFT JOIN rev.brief_unit_media bumed ON bumed.brief_unit_id = bu.id
WHERE b.id = ANY($1::UUID[])
ORDER BY b.id, bu.id;
