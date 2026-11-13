-- Migration 27: Asset content embeddings
-- Semantic-chunk vector storage for extracted asset content, mirroring the
-- shape of rev.guide_content_embeddings but scoped to assets. Used by
-- `asset search` and unified retrieval in the conduit prompt pipeline.
--
-- Uses the same ModernBERT embedding service conduit already runs (768-dim
-- Matryoshka truncation). IVFFlat index for cosine similarity to match the
-- guide_content_embeddings convention; rebuild as HNSW later if recall needs
-- improvement.

-- ============================================================================
-- rev.asset_content_embeddings — One row per semantic chunk
-- ============================================================================
CREATE TABLE rev.asset_content_embeddings (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    asset_id UUID NOT NULL REFERENCES rev.assets (id),
    extraction_id UUID NOT NULL REFERENCES rev.asset_extractions (id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    chunk_text TEXT NOT NULL,
    embedding vector(768) NOT NULL,
    source_position JSONB NOT NULL DEFAULT '{}'::jsonb,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX asset_content_embeddings_asset_idx
    ON rev.asset_content_embeddings (asset_id);
CREATE INDEX asset_content_embeddings_extraction_idx
    ON rev.asset_content_embeddings (extraction_id);
CREATE INDEX asset_content_embeddings_chunk_idx
    ON rev.asset_content_embeddings (extraction_id, chunk_index);

-- Vector similarity index — IVFFlat cosine to match guide_content_embeddings.
-- NOTE: IVFFlat requires data to exist before building. For initial deploys
-- with < 1000 rows the index will be under-trained; REINDEX CONCURRENTLY
-- after initial data load or switch to HNSW if recall matters.
CREATE INDEX asset_content_embeddings_vector_idx
    ON rev.asset_content_embeddings
    USING ivfflat (embedding vector_cosine_ops)
    WITH (lists = 100);
