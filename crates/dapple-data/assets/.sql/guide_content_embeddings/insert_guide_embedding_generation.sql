INSERT INTO rev.guide_embedding_generations (guide_id)
VALUES ($1)
RETURNING id, guide_id, created_at;
