-- Migration 26: Asset extractions
-- Dedicated storage model for content extracted from binary assets (PDF, docx).
-- Extractions are properties of an asset, not new assets themselves. They reuse
-- the chunk store (rev.chunks) via rev.asset_extraction_chunks for byte storage,
-- and carry a separate position map in rev.asset_extraction_positions.
--
-- Append-only: re-extraction creates new rows keyed by
-- (asset_id, extractor_kind, extractor_version). Old rows stay; `is_current` is
-- maintained by the writer so lookups can filter quickly.

-- ============================================================================
-- rev.asset_extractions — One row per (asset, extractor, version)
-- ============================================================================
CREATE TABLE rev.asset_extractions (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    asset_id UUID NOT NULL REFERENCES rev.assets (id),
    extractor_kind TEXT NOT NULL,
    extractor_version INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    text_length BIGINT,
    page_count INTEGER,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    error_message TEXT,
    is_current BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    CONSTRAINT asset_extractions_status_check
        CHECK (status IN ('pending', 'running', 'ok', 'failed')),
    UNIQUE (asset_id, extractor_kind, extractor_version)
);

CREATE INDEX asset_extractions_asset_idx
    ON rev.asset_extractions (asset_id);
CREATE INDEX asset_extractions_kind_version_idx
    ON rev.asset_extractions (extractor_kind, extractor_version);
CREATE INDEX asset_extractions_current_idx
    ON rev.asset_extractions (asset_id, extractor_kind)
    WHERE is_current;
CREATE INDEX asset_extractions_status_idx
    ON rev.asset_extractions (status)
    WHERE status IN ('pending', 'running');

-- ============================================================================
-- rev.asset_extraction_chunks — Associative table: extraction <-> chunks
-- Byte-aligned storage chunks for the extracted text, reusing rev.chunks.
-- Ordinal is byte-range assembly position (semantic).
-- ============================================================================
CREATE TABLE rev.asset_extraction_chunks (
    extraction_id UUID NOT NULL REFERENCES rev.asset_extractions (id) ON DELETE CASCADE,
    chunk_id UUID NOT NULL REFERENCES rev.chunks (id),
    ordinal INTEGER NOT NULL,
    PRIMARY KEY (extraction_id, chunk_id)
);

CREATE INDEX asset_extraction_chunks_extraction_idx
    ON rev.asset_extraction_chunks (extraction_id);
CREATE INDEX asset_extraction_chunks_chunk_idx
    ON rev.asset_extraction_chunks (chunk_id);
CREATE INDEX asset_extraction_chunks_ordinal_idx
    ON rev.asset_extraction_chunks (extraction_id, ordinal);

-- ============================================================================
-- rev.asset_extraction_positions — Position map (page/paragraph/etc.) per chunk
-- Normalized so semantic chunking (embeddings) can reference the same positions
-- independently. source_position JSONB carries extractor-specific shapes:
--   PDF  → {"page": 3}
--   docx → {"paragraph": 42}
-- ============================================================================
CREATE TABLE rev.asset_extraction_positions (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    extraction_id UUID NOT NULL REFERENCES rev.asset_extractions (id) ON DELETE CASCADE,
    byte_offset BIGINT NOT NULL,
    byte_length BIGINT NOT NULL,
    source_position JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX asset_extraction_positions_extraction_idx
    ON rev.asset_extraction_positions (extraction_id);
CREATE INDEX asset_extraction_positions_offset_idx
    ON rev.asset_extraction_positions (extraction_id, byte_offset);
