WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],            $2::UUID[]         ) WITH ORDINALITY AS o(
        block_id,
        fluent_message_id,
        ord
    )
)
UPDATE rev.guide_blocks gb
SET fluent_message_id = args.fluent_message_id
FROM args
WHERE gb.id = args.block_id
  AND gb.fluent_message_id IS NULL;
