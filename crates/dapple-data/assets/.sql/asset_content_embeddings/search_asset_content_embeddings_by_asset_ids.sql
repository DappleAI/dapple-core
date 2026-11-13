SELECT
    e.id,
    e.asset_id,
    e.extraction_id,
    e.chunk_index,
    e.chunk_text,
    e.source_position,
    e.metadata,
    (1 - (e.embedding <=> $1::vector(768)))::FLOAT8 AS similarity
FROM rev.asset_content_embeddings e
WHERE (
    COALESCE(array_length($2::UUID[], 1), 0) = 0
    OR e.asset_id = ANY($2::UUID[])
)
ORDER BY e.embedding <=> $1::vector(768)
LIMIT $3;
