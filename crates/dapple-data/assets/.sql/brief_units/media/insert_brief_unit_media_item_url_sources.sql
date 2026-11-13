WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[], $2::TEXT[])
    WITH ORDINALITY AS o(media_item_id, url, ord)
)
INSERT INTO rev.brief_unit_media_item_url_sources (media_item_id, url)
SELECT args.media_item_id, args.url
FROM args
ORDER BY args.ord
RETURNING media_item_id, url;
