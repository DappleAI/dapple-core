SELECT
    fmb.fluent_message_id,
    fml.tag AS locale_tag,
    fmb.body
FROM rev.fluent_message_bodies fmb
JOIN rev.fluent_message_locales fml ON fml.id = fmb.locale_id
WHERE fmb.fluent_message_id = ANY($1::UUID[])
ORDER BY fmb.fluent_message_id, fml.tag;
