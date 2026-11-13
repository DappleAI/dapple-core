WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],              $2::SMALLINT[],          $3::TEXT[],               $4::TEXT[],               $5::JSONB[],              $6::UUID[]            ) WITH ORDINALITY AS o(
        chapter_id,
        directive_type_id,
        name,
        content,
        attributes,
        fluent_message_id,
        ord
    )
),
new_blocks AS (
    INSERT INTO rev.guide_blocks (id, chapter_id, directive_type_id, name, content, attributes, fluent_message_id)
    SELECT
        uuidv7(),
        args.chapter_id,
        args.directive_type_id,
        args.name,
        args.content,
        args.attributes,
        args.fluent_message_id
    FROM args
    ORDER BY args.ord
    RETURNING id, chapter_id, directive_type_id, name, content, attributes, fluent_message_id, created_at
)
SELECT
    nb.id,
    nb.chapter_id,
    nb.directive_type_id,
    dt.key AS directive_type_key,
    nb.name,
    nb.content,
    nb.attributes,
    nb.fluent_message_id,
    nb.created_at
FROM new_blocks nb
JOIN rev.guide_block_directive_types dt ON dt.id = nb.directive_type_id
ORDER BY nb.id;
