-- =============================================================================
-- Guide Resources and Block Citations
-- =============================================================================
-- Introduces guide-level resource references (PDFs, URLs) and block-level
-- citations that link prose blocks to specific resource pages.
-- =============================================================================

-- ============================================================================
-- rev.guide_resources — Resource references attached to a guide version
-- ============================================================================
CREATE TABLE rev.guide_resources (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    guide_id UUID NOT NULL REFERENCES rev.guides (id),
    asset_id UUID REFERENCES rev.assets (id),
    key TEXT NOT NULL,
    title TEXT,
    author TEXT,
    url TEXT,
    retrieved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (guide_id, key)
);

CREATE INDEX guide_resources_guide_idx ON rev.guide_resources (guide_id);
CREATE INDEX guide_resources_asset_idx ON rev.guide_resources (asset_id)
    WHERE asset_id IS NOT NULL;

-- ============================================================================
-- rev.guide_block_citations — Block-level citations referencing resources
-- ============================================================================
CREATE TABLE rev.guide_block_citations (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    block_id UUID NOT NULL REFERENCES rev.guide_blocks (id),
    resource_id UUID NOT NULL REFERENCES rev.guide_resources (id),
    pages TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX guide_block_citations_block_idx ON rev.guide_block_citations (block_id);
CREATE INDEX guide_block_citations_resource_idx ON rev.guide_block_citations (resource_id);
