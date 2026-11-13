WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[],
        $3::INTEGER[],
        $4::TEXT[],
        $5::vector(768)[],
        $6::JSONB[],
        $7::JSONB[]
    ) WITH ORDINALITY AS o(
        asset_id, extraction_id, chunk_index, chunk_text, embedding,
        source_position, metadata, ord
    )
),
new_embeddings AS (
    INSERT INTO rev.asset_content_embeddings (
        id,
        asset_id,
        extraction_id,
        chunk_index,
        chunk_text,
        embedding,
        source_position,
        metadata
    )
    SELECT
        uuidv7(),
        args.asset_id,
        args.extraction_id,
        args.chunk_index,
        args.chunk_text,
        args.embedding,
        args.source_position,
        args.metadata
    FROM args
    ORDER BY args.ord
    RETURNING id, asset_id, extraction_id, chunk_index, created_at
)
SELECT
    ne.id,
    ne.asset_id,
    ne.extraction_id,
    ne.chunk_index,
    ne.created_at
FROM new_embeddings ne
ORDER BY ne.id;
