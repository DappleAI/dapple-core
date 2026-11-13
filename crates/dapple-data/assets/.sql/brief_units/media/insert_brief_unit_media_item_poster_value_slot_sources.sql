WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[], $2::UUID[])
    WITH ORDINALITY AS o(poster_id, value_slot_id, ord)
)
INSERT INTO rev.brief_unit_media_item_poster_value_slot_sources (poster_id, value_slot_id)
SELECT args.poster_id, args.value_slot_id
FROM args
ORDER BY args.ord
RETURNING poster_id, value_slot_id;
