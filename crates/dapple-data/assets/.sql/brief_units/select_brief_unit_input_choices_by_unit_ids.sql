SELECT buic.id, buic.brief_unit_id,
       buic.label_text, buic.label_fluent_message_id AS "label_fluent_message_id?",
       buic.value_text, buic.created_at
FROM rev.brief_unit_input_choices buic
WHERE buic.brief_unit_id = ANY($1::UUID[])
ORDER BY buic.brief_unit_id, buic.id;
