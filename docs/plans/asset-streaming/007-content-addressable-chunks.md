# Plan 007: Content-Addressable Chunk Storage

## Overview

Add a `chunk_checksums` lookup table for content-addressable deduplication. Deduplication happens at write time (when chunks are appended to drafts), not at finalization. The `draft_chunks` table stores references to already-deduplicated chunks rather than raw data.

## Schema Changes

### chunks (Unchanged)

```sql
CREATE TABLE rev.chunks (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    data BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chunk_size CHECK (octet_length(data) <= 262144)
);
```

### chunk_checksums (New)

```sql
CREATE TABLE rev.chunk_checksums (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    checksum BYTEA NOT NULL,  -- SHA256 (32 bytes)
    chunk_id BIGINT NOT NULL REFERENCES rev.chunks (id)
);

CREATE INDEX idx_chunk_checksums_checksum ON rev.chunk_checksums (checksum);
CREATE INDEX idx_chunk_checksums_chunk_id ON rev.chunk_checksums (chunk_id);
```

- Proper BIGINT PK (stable identity)
- No UNIQUE on checksum (bad checksum doesn't block correct one)
- No UNIQUE on chunk_id (integrity service handles duplicates)

### draft_chunks (Modified)

**Before:**
```sql
CREATE TABLE rev.draft_chunks (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    draft_id BIGINT NOT NULL REFERENCES rev.drafts (id),
    data BYTEA NOT NULL,  -- raw bytes
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

**After:**
```sql
CREATE TABLE rev.draft_chunks (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    draft_id BIGINT NOT NULL REFERENCES rev.drafts (id),
    chunk_id BIGINT NOT NULL REFERENCES rev.chunks (id),
    ordinal INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_draft_chunks_draft_id ON rev.draft_chunks (draft_id);
```

- `data BYTEA` → `chunk_id BIGINT` (reference to deduplicated chunk)
- Added `ordinal` for ordering

### asset_chunks (Unchanged)

```sql
CREATE TABLE rev.asset_chunks (
    asset_id BIGINT NOT NULL REFERENCES rev.assets (id),
    chunk_id BIGINT NOT NULL REFERENCES rev.chunks (id),
    ordinal INTEGER NOT NULL,
    PRIMARY KEY (asset_id, chunk_id)
);
```

## Insert Draft Chunk (Dedup at Write Time)

Single query — checksum computed inline, dedup via INSERT SELECT:

```sql
WITH input AS (
    SELECT
        $1::BYTEA as data,
        sha256($1::BYTEA) as checksum,
        $2::BIGINT as draft_id,
        $3::INTEGER as ordinal
),
existing AS (
    SELECT cc.chunk_id, i.checksum
    FROM input i
    JOIN rev.chunk_checksums cc ON cc.checksum = i.checksum
    LIMIT 1
),
new_chunk AS (
    INSERT INTO rev.chunks (data)
    SELECT i.data
    FROM input i
    WHERE NOT EXISTS (SELECT 1 FROM existing)
    RETURNING id
),
new_checksum AS (
    INSERT INTO rev.chunk_checksums (checksum, chunk_id)
    SELECT i.checksum, nc.id
    FROM input i, new_chunk nc
),
chunk_ref AS (
    SELECT chunk_id FROM existing
    UNION ALL
    SELECT id FROM new_chunk
)
INSERT INTO rev.draft_chunks (draft_id, chunk_id, ordinal)
SELECT i.draft_id, cr.chunk_id, i.ordinal
FROM input i, chunk_ref cr
RETURNING id, draft_id, chunk_id, ordinal;
```

- Checksum computed via `sha256()` function, not passed as bind variable
- If existing chunk found: reuse it
- If new: insert chunk + checksum record
- Draft_chunk links to the (possibly reused) chunk

## Finalization (Simplified)

Just copies draft_chunks references to asset_chunks:

```sql
WITH draft_info AS (
    SELECT d.id as draft_id, d.object_id, d.asset_kind_id, d.mime_type
    FROM rev.drafts d
    WHERE d.object_id = $1
),
-- Calculate content length from referenced chunks
content_stats AS (
    SELECT
        COUNT(*)::BIGINT as chunk_count,
        COALESCE(SUM(octet_length(c.data)), 0)::BIGINT as total_length
    FROM rev.draft_chunks dc
    JOIN draft_info di ON di.draft_id = dc.draft_id
    JOIN rev.chunks c ON c.id = dc.chunk_id
),
-- Create path, version, asset (same as before)
...
-- Link asset to chunks (simple copy from draft_chunks)
new_links AS (
    INSERT INTO rev.asset_chunks (asset_id, chunk_id, ordinal)
    SELECT na.id, dc.chunk_id, dc.ordinal
    FROM new_asset na
    JOIN draft_info di ON TRUE
    JOIN rev.draft_chunks dc ON dc.draft_id = di.draft_id
    ORDER BY dc.ordinal
    RETURNING asset_id, chunk_id
)
SELECT ...;
```

No checksum computation at finalization — already done at write time.

## Read Pattern (Unchanged)

```sql
SELECT c.data
FROM rev.asset_chunks ac
JOIN rev.chunks c ON c.id = ac.chunk_id
WHERE ac.asset_id = $1
ORDER BY ac.ordinal;
```

No join to `chunk_checksums` — checksums only used for dedup during writes.

## Integrity Validation

Background service checks for:

1. **Missing checksums**: Chunks without a checksum record
2. **Bad checksums**: Checksum doesn't match `sha256(data)`
3. **Duplicate checksums**: Multiple records for same checksum (cleanup)
4. **Missing chunks**: Asset/draft references chunk that doesn't exist

```sql
-- Chunks missing checksum records
SELECT c.id, c.object_id
FROM rev.chunks c
LEFT JOIN rev.chunk_checksums cc ON cc.chunk_id = c.id
WHERE cc.checksum IS NULL;

-- Checksum mismatches
SELECT cc.chunk_id, cc.checksum, sha256(c.data) as computed
FROM rev.chunk_checksums cc
JOIN rev.chunks c ON c.id = cc.chunk_id
WHERE cc.checksum != sha256(c.data);

-- Assets with integrity issues → quarantine
SELECT DISTINCT a.object_id
FROM rev.assets a
JOIN rev.asset_chunks ac ON ac.asset_id = a.id
LEFT JOIN rev.chunks c ON c.id = ac.chunk_id
LEFT JOIN rev.chunk_checksums cc ON cc.chunk_id = c.id
WHERE c.id IS NULL           -- missing chunk
   OR cc.checksum IS NULL    -- missing checksum
   OR cc.checksum != sha256(c.data);  -- bad checksum
```

Quarantine = soft flag on asset, excluded from content serving.

## Migration

```sql
-- 1. Create checksum lookup table
CREATE TABLE rev.chunk_checksums (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    checksum BYTEA NOT NULL,
    chunk_id BIGINT NOT NULL REFERENCES rev.chunks (id)
);

-- 2. Populate checksums for existing chunks
INSERT INTO rev.chunk_checksums (checksum, chunk_id)
SELECT sha256(data), id
FROM rev.chunks;

-- 3. Indexes
CREATE INDEX idx_chunk_checksums_checksum ON rev.chunk_checksums (checksum);
CREATE INDEX idx_chunk_checksums_chunk_id ON rev.chunk_checksums (chunk_id);

-- 4. Migrate draft_chunks: add chunk_id, ordinal; populate from existing data
ALTER TABLE rev.draft_chunks ADD COLUMN chunk_id BIGINT REFERENCES rev.chunks (id);
ALTER TABLE rev.draft_chunks ADD COLUMN ordinal INTEGER;

-- 5. For existing draft_chunks with data, create chunks and link
WITH existing_draft_chunks AS (
    SELECT id, draft_id, data, sha256(data) as checksum,
           ROW_NUMBER() OVER (PARTITION BY draft_id ORDER BY id) as ordinal
    FROM rev.draft_chunks
    WHERE data IS NOT NULL
),
new_chunks AS (
    INSERT INTO rev.chunks (data)
    SELECT DISTINCT ON (checksum) data
    FROM existing_draft_chunks
    RETURNING id, sha256(data) as checksum
),
new_checksums AS (
    INSERT INTO rev.chunk_checksums (checksum, chunk_id)
    SELECT checksum, id FROM new_chunks
)
UPDATE rev.draft_chunks dc
SET chunk_id = nc.id, ordinal = edc.ordinal
FROM existing_draft_chunks edc
JOIN new_chunks nc ON nc.checksum = edc.checksum
WHERE dc.id = edc.id;

-- 6. Drop data column, add NOT NULL constraints
ALTER TABLE rev.draft_chunks DROP COLUMN data;
ALTER TABLE rev.draft_chunks ALTER COLUMN chunk_id SET NOT NULL;
ALTER TABLE rev.draft_chunks ALTER COLUMN ordinal SET NOT NULL;
```

## File Locations

```
dapple-data/
├── migrations/
│   └── YYYYMMDD_chunk_checksums.sql
├── assets/sql/
│   ├── drafts/
│   │   ├── insert_draft_chunk.sql   -- New: dedup at write time
│   │   └── finalize_draft.sql       -- Simplified: just copy refs
│   └── chunk_checksums/
│       └── (no separate queries needed - all inline)
└── src/
    ├── model/
    │   └── drafts.rs                -- Update InsertDraftChunkArgs
    └── query/
        └── drafts.rs                -- Update insert_draft_chunks
```

## Rust Changes

### InsertDraftChunkArgs

```rust
// Before
pub struct InsertDraftChunkArgs {
    pub draft_ids: Vec<i64>,
    pub data: Vec<Vec<u8>>,
}

// After
pub struct InsertDraftChunkArgs {
    pub draft_ids: Vec<i64>,
    pub data: Vec<Vec<u8>>,      // Raw bytes passed to SQL
    pub ordinals: Vec<i32>,
}
```

The SQL computes checksums and handles dedup — Rust just passes the raw bytes.

## Summary

- **Dedup at write time**: Checksum computed and checked when chunk appended to draft
- **draft_chunks stores references**: `chunk_id` instead of `data BYTEA`
- **Finalization simplified**: Just copies draft_chunks → asset_chunks
- **No UNIQUE constraints**: Integrity service handles duplicates
- **Read path untouched**: No performance impact on content serving
