-- Insert caption references for media items
-- Parameters:
--   $1: media_item_ids (UUID[])
--   $2: fluent_message_ids (UUID[])
WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[], $2::UUID[])
    WITH ORDINALITY AS o(media_item_id, fluent_message_id, ord)
)
INSERT INTO rev.brief_unit_media_item_captions (media_item_id, fluent_message_id)
SELECT args.media_item_id, args.fluent_message_id
FROM args
ORDER BY args.ord
RETURNING media_item_id, fluent_message_id;
