WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[], $2::UUID[])
    WITH ORDINALITY AS o(media_item_id, asset_id, ord)
)
INSERT INTO rev.brief_unit_media_item_asset_sources (media_item_id, asset_id)
SELECT args.media_item_id, args.asset_id
FROM args
ORDER BY args.ord
RETURNING media_item_id, asset_id;
