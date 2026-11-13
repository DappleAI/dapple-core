-- Migration 16: pgvector extension and guide content embeddings
-- Enables vector similarity search for guide content.
-- Uses the existing rev.fluent_message_locales table for locale references.

-- ============================================================================
-- Enable pgvector extension
-- ============================================================================
CREATE EXTENSION IF NOT EXISTS vector;

-- ============================================================================
-- rev.guide_content_embeddings — One row per embedded block
-- ============================================================================
CREATE TABLE rev.guide_content_embeddings (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    guide_id UUID NOT NULL REFERENCES rev.guides (id),
    chapter_id UUID NOT NULL REFERENCES rev.guide_chapters (id),
    block_id UUID NOT NULL REFERENCES rev.guide_blocks (id),
    directive_type_id SMALLINT NOT NULL REFERENCES rev.guide_block_directive_types (id),
    chunk_text TEXT NOT NULL,
    embedding vector(768) NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

-- FK indexes
CREATE INDEX guide_content_embeddings_guide_idx
    ON rev.guide_content_embeddings (guide_id);
CREATE INDEX guide_content_embeddings_chapter_idx
    ON rev.guide_content_embeddings (chapter_id);
CREATE INDEX guide_content_embeddings_block_idx
    ON rev.guide_content_embeddings (block_id);
CREATE INDEX guide_content_embeddings_directive_type_idx
    ON rev.guide_content_embeddings (directive_type_id);

-- Vector similarity index (IVFFlat with cosine distance)
-- NOTE: IVFFlat requires data to exist before building the index.
-- For initial deployment with < 1000 rows, this index will be under-trained.
-- Consider rebuilding with `REINDEX INDEX CONCURRENTLY` after initial data load,
-- or switching to HNSW (which does not require pre-existing data) if recall
-- needs improvement.
CREATE INDEX guide_content_embeddings_vector_idx
    ON rev.guide_content_embeddings
    USING ivfflat (embedding vector_cosine_ops)
    WITH (lists = 100);

-- ============================================================================
-- rev.guide_embedding_tags — Junction: embedding <-> guide_tags
-- ============================================================================
CREATE TABLE rev.guide_embedding_tags (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    embedding_id UUID NOT NULL REFERENCES rev.guide_content_embeddings (id) ON DELETE CASCADE,
    tag_id SMALLINT NOT NULL REFERENCES rev.guide_tags (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (embedding_id, tag_id)
);

CREATE INDEX guide_embedding_tags_embedding_idx
    ON rev.guide_embedding_tags (embedding_id);
CREATE INDEX guide_embedding_tags_tag_idx
    ON rev.guide_embedding_tags (tag_id);

-- ============================================================================
-- rev.guide_embedding_locales — Junction: embedding <-> fluent_message_locales
-- ============================================================================
CREATE TABLE rev.guide_embedding_locales (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    embedding_id UUID NOT NULL REFERENCES rev.guide_content_embeddings (id) ON DELETE CASCADE,
    locale_id SMALLINT NOT NULL REFERENCES rev.fluent_message_locales (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (embedding_id, locale_id)
);

CREATE INDEX guide_embedding_locales_embedding_idx
    ON rev.guide_embedding_locales (embedding_id);
CREATE INDEX guide_embedding_locales_locale_idx
    ON rev.guide_embedding_locales (locale_id);
