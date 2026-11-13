WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[], $2::TEXT[])
    WITH ORDINALITY AS o(poster_id, url, ord)
)
INSERT INTO rev.brief_unit_media_item_poster_url_sources (poster_id, url)
SELECT args.poster_id, args.url
FROM args
ORDER BY args.ord
RETURNING poster_id, url;
