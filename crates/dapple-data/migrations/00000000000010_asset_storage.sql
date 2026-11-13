-- Migration 10: Asset storage
-- drafts, chunks, asset_chunks (chunked binary storage)

-- ============================================================================
-- rev.drafts — In-progress content metadata
-- ============================================================================
CREATE TABLE rev.drafts (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    asset_kind_id SMALLINT NOT NULL REFERENCES rev.asset_kinds (id),
    mime_type TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX drafts_asset_kind_idx ON rev.drafts (asset_kind_id);

-- ============================================================================
-- rev.chunks — Immutable finalized binary data (256KB max)
-- ============================================================================
CREATE TABLE rev.chunks (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    data BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chunk_size CHECK (octet_length(data) <= 262144)
);

-- ============================================================================
-- rev.chunk_checksums — Content-addressable storage with SHA-256
-- ============================================================================
CREATE TABLE rev.chunk_checksums (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    checksum BYTEA NOT NULL,
    chunk_id UUID NOT NULL REFERENCES rev.chunks (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX chunk_checksums_checksum_idx ON rev.chunk_checksums (checksum);
CREATE INDEX chunk_checksums_chunk_idx ON rev.chunk_checksums (chunk_id);

-- ============================================================================
-- rev.draft_chunks — Append-only during draft creation
-- ============================================================================
CREATE TABLE rev.draft_chunks (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    draft_id UUID NOT NULL REFERENCES rev.drafts (id),
    chunk_id UUID NOT NULL REFERENCES rev.chunks (id),
    ordinal INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX draft_chunks_draft_idx ON rev.draft_chunks (draft_id);
CREATE INDEX draft_chunks_chunk_idx ON rev.draft_chunks (chunk_id);
CREATE INDEX draft_chunks_draft_ordinal_idx ON rev.draft_chunks (draft_id, ordinal);

-- ============================================================================
-- rev.assets — Versioned metadata for binary content
-- ============================================================================
CREATE TABLE rev.assets (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    draft_id UUID REFERENCES rev.drafts (id),
    content_length BIGINT NOT NULL,
    mime_type TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX assets_versioned_object_idx ON rev.assets (versioned_object_id);
CREATE INDEX assets_draft_idx ON rev.assets (draft_id) WHERE draft_id IS NOT NULL;

-- ============================================================================
-- rev.asset_chunks — Associative (Asset <-> Chunks, ordered)
-- ordinal is semantic here (byte-range assembly position)
-- ============================================================================
CREATE TABLE rev.asset_chunks (
    asset_id UUID NOT NULL REFERENCES rev.assets (id),
    chunk_id UUID NOT NULL REFERENCES rev.chunks (id),
    ordinal INTEGER NOT NULL,
    PRIMARY KEY (asset_id, chunk_id)
);

CREATE INDEX asset_chunks_asset_idx ON rev.asset_chunks (asset_id);
CREATE INDEX asset_chunks_chunk_idx ON rev.asset_chunks (chunk_id);

-- ============================================================================
-- Add asset_id FK to brief_unit_media_item_asset_sources (deferred from migration 8)
-- ============================================================================
-- Note: asset_id columns in brief_unit_media tables were created without FK
-- constraints in migration 8 because rev.assets didn't exist yet.
-- The FK to rev.assets is enforced at application level.

-- Similarly for poster_asset_sources
