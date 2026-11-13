-- Insert asset sources for media item posters
-- Parameters:
--   $1: poster_ids (UUID[])
--   $2: asset_ids (UUID[])
WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[], $2::UUID[])
    WITH ORDINALITY AS o(poster_id, asset_id, ord)
)
INSERT INTO rev.brief_unit_media_item_poster_asset_sources (poster_id, asset_id)
SELECT args.poster_id, args.asset_id
FROM args
ORDER BY args.ord
RETURNING poster_id, asset_id;
