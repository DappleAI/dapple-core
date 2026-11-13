WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],                  $2::TEXT[],                   $3::UUID[],                   $4::TEXT[],                   $5::TIMESTAMPTZ[],           $6::TEXT[],                   $7::TEXT[]                ) WITH ORDINALITY AS o(
        asset_id,
        author,
        guide_id,
        key,
        retrieved_at,
        title,
        url,
        ord
    )
),
objs AS (
    INSERT INTO rev.guide_resources (guide_id, asset_id, key, title, author, url, retrieved_at)
    SELECT
        args.guide_id,
        args.asset_id,
        args.key,
        args.title,
        args.author,
        args.url,
        args.retrieved_at
    FROM args
    ORDER BY args.ord
    RETURNING id, guide_id, key, created_at
)
SELECT * FROM objs ORDER BY objs.id;
