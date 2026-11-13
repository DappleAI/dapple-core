WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],             $2::TEXT[],              $3::UUID[]           ) WITH ORDINALITY AS o(
        block_id,
        pages,
        resource_id,
        ord
    )
),
objs AS (
    INSERT INTO rev.guide_block_citations (block_id, resource_id, pages)
    SELECT args.block_id, args.resource_id, args.pages
    FROM args
    ORDER BY args.ord
    RETURNING id, block_id, resource_id, pages, created_at
)
SELECT * FROM objs ORDER BY objs.id;
