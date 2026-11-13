-- Similarity search scoped to specific guides (for agent RAG).
-- Only searches embeddings from the latest generation per guide.
-- $1: query embedding (vector(768))
-- $2: guide_ids (UUID[])
-- $3: tag keys (TEXT[])
-- $4: locale tag (TEXT)
-- $5: limit (INT)
WITH latest_gens AS (
    SELECT DISTINCT ON (guide_id) id AS generation_id
    FROM rev.guide_embedding_generations
    WHERE guide_id = ANY($2::UUID[])
    ORDER BY guide_id, created_at DESC
)
SELECT
    e.id,
    e.guide_id,
    e.chapter_id,
    e.block_id,
    dt.key AS directive_type_key,
    e.chunk_text,
    e.metadata,
    1 - (e.embedding <=> $1::vector(768)) AS similarity
FROM rev.guide_content_embeddings e
JOIN latest_gens lg ON lg.generation_id = e.generation_id
JOIN rev.guide_block_directive_types dt ON dt.id = e.directive_type_id
JOIN rev.guide_embedding_tags et ON et.embedding_id = e.id
JOIN rev.guide_embedding_locales el ON el.embedding_id = e.id
JOIN rev.guide_tags gt ON gt.id = et.tag_id
JOIN rev.fluent_message_locales fml ON fml.id = el.locale_id
WHERE e.guide_id = ANY($2::UUID[])
AND gt.key = ANY($3::TEXT[])
AND fml.tag = $4
ORDER BY e.embedding <=> $1::vector(768)
LIMIT $5;
