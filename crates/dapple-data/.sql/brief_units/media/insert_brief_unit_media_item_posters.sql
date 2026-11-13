-- Insert poster records for media items
-- Parameters:
--   $1: media_item_ids (UUID[])
--   $2: source_type_keys (TEXT[]) - 'asset' or 'url'
WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[], $2::TEXT[])
    WITH ORDINALITY AS o(media_item_id, source_type_key, ord)
),
source_types AS (
    SELECT st.id, st.key, args.media_item_id, args.ord
    FROM args
    JOIN rev.brief_media_source_types st ON st.key = args.source_type_key
),
objs AS (
    INSERT INTO rev.brief_unit_media_item_posters (media_item_id, source_type_id)
    SELECT st.media_item_id, st.id
    FROM source_types st
    ORDER BY st.ord
    RETURNING id, media_item_id, source_type_id, created_at
)
SELECT
    objs.id,
    objs.media_item_id,
    st.key as source_type_key
FROM objs
JOIN rev.brief_media_source_types st ON st.id = objs.source_type_id
ORDER BY objs.id;
