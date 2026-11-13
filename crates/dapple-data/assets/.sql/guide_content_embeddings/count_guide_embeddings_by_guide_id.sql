SELECT count(*)
FROM rev.guide_content_embeddings e
WHERE e.guide_id = $1
AND e.generation_id = (
    SELECT g.id FROM rev.guide_embedding_generations g
    WHERE g.guide_id = $1
    ORDER BY g.created_at DESC
    LIMIT 1
)
