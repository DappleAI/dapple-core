WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],               $2::UUID[],               $3::UUID[],               $4::SMALLINT[],           $5::TEXT[],                $6::vector(768)[],         $7::JSONB[],               $8::UUID[]             ) WITH ORDINALITY AS o(
        guide_id, chapter_id, block_id, directive_type_id,
        chunk_text, embedding, metadata, generation_id, ord
    )
),
new_embeddings AS (
    INSERT INTO rev.guide_content_embeddings (
        id, guide_id, chapter_id, block_id,
        directive_type_id, chunk_text, embedding, metadata, generation_id
    )
    SELECT
        uuidv7(),
        args.guide_id,
        args.chapter_id,
        args.block_id,
        args.directive_type_id,
        args.chunk_text,
        args.embedding,
        args.metadata,
        args.generation_id
    FROM args
    ORDER BY args.ord
    RETURNING id, guide_id, chapter_id, block_id, directive_type_id, created_at
)
SELECT
    ne.id,
    ne.guide_id,
    ne.chapter_id,
    ne.block_id,
    ne.directive_type_id,
    ne.created_at
FROM new_embeddings ne
ORDER BY ne.id;
