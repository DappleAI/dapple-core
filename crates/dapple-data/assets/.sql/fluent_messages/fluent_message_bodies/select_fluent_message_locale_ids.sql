SELECT id, tag
FROM rev.fluent_message_locales
WHERE tag = ANY($1::TEXT[]);
