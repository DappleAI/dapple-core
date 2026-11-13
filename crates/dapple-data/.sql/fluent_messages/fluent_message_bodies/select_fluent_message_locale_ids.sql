-- Resolve locale tags to locale IDs.
-- Parameters: $1 locale tags (TEXT[])
SELECT id, tag
FROM rev.fluent_message_locales
WHERE tag = ANY($1::TEXT[]);
