WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],              $2::SMALLINT[]       ) WITH ORDINALITY AS o(
        block_id,
        tag_id,
        ord
    )
)
INSERT INTO rev.guide_block_tags (id, block_id, tag_id)
SELECT uuidv7(), args.block_id, args.tag_id
FROM args
ORDER BY args.ord
RETURNING id, block_id, tag_id, created_at;
