-- Migration 81: dataset (spreadsheet / tabular) schema extractions.
--
-- Mirrors rev.asset_extractions (Migration 26) but stores a compact schema
-- ENVELOPE inline as JSONB rather than chunk-backed full text. The raw rows are
-- NOT copied here — the source of truth for rows stays the asset's own chunks
-- (rev.asset_chunks), re-parsed on demand by `dataset open`.
--
-- Formally append-only (bindings pin extraction rows by FK, see Migration 82):
-- the envelope / content columns are write-once (set at completion) and
-- immutable thereafter; re-extraction inserts a NEW row keyed by
-- (asset_id, extractor_version); a referenced row is NEVER deleted (a delete
-- would dangle a real FK). There is NO is_current flag — "current" is derived
-- as the latest row per asset (ORDER BY id DESC; UUIDv7 id is creation-ordered),
-- which eliminates the flag-demotion UPDATE entirely.
--
-- See docs/plans/spreadsheet-dataset-ingest.md (Section 4.2 / 4.4).
CREATE TABLE rev.asset_dataset_extractions (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    asset_id UUID NOT NULL REFERENCES rev.assets (id),
    extractor_version INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    -- Compact schema envelope (see Section 4.4). Row DATA is NOT stored here.
    -- Write-once (set at completion) and immutable thereafter; re-extraction = new row.
    envelope JSONB NOT NULL DEFAULT '{}'::jsonb,
    table_count INTEGER,
    row_count BIGINT,             -- total across tables, for cheap display
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    CONSTRAINT asset_dataset_extractions_status_check
        CHECK (status IN ('pending', 'running', 'ok', 'failed')),
    UNIQUE (asset_id, extractor_version)
);

-- "Current" extraction = latest row per asset (ORDER BY id DESC, UUIDv7);
-- no is_current flag, no flag-demotion UPDATE.
CREATE INDEX asset_dataset_extractions_asset_idx
    ON rev.asset_dataset_extractions (asset_id);
CREATE INDEX asset_dataset_extractions_status_idx
    ON rev.asset_dataset_extractions (status)
    WHERE status IN ('pending', 'running');
