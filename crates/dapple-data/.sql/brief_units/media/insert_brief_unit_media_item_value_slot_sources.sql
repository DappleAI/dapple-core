-- Insert value_slot sources for media items
-- Parameters:
--   $1: media_item_ids (UUID[])
--   $2: value_slot_ids (UUID[])
WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[], $2::UUID[])
    WITH ORDINALITY AS o(media_item_id, value_slot_id, ord)
)
INSERT INTO rev.brief_unit_media_item_value_slot_sources (media_item_id, value_slot_id)
SELECT args.media_item_id, args.value_slot_id
FROM args
ORDER BY args.ord
RETURNING media_item_id, value_slot_id;
