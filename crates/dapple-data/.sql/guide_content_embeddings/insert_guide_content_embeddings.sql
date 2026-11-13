-- Insert guide content embeddings.
-- Vectors are passed as float4[] arrays (one per embedding row).
-- pgvector accepts float4[] as implicit cast to vector(768).
-- Each batch belongs to a generation_id for append-only versioning.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],       -- guide_id[]
        $2::UUID[],       -- chapter_id[]
        $3::UUID[],       -- block_id[]
        $4::SMALLINT[],   -- directive_type_id[]
        $5::TEXT[],        -- chunk_text[]
        $6::vector(768)[], -- embedding[]
        $7::JSONB[],       -- metadata[]
        $8::UUID[]         -- generation_id[]
    ) WITH ORDINALITY AS o(
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
