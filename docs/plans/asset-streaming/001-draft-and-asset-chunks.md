# Plan: Draft and Asset Chunks Storage

## Overview

Replace the current `assets.contents BYTEA` column with a chunked storage model that supports:
- Streaming writes during content generation (TTS, uploads)
- Append-only immutability for audit trail integrity
- Draft state before finalization
- 256KB chunk size for efficient streaming

## Core Requirement: Audit Trail Integrity

Every asset sent to an LLM must be provably the exact bytes that were sent. This means:
- No in-place updates to chunks
- No caching tricks that could create ambiguity
- Clear provenance from draft to finalized asset

## Data Model

### Draft Phase (Content Under Construction)

```
drafts (metadata for in-progress content)
    │
    └── draft_chunks (ordered binary chunks, append-only)
            ├── chunk id=1: bytes 0-262143
            ├── chunk id=2: bytes 262144-524287
            └── chunk id=3: bytes 524288-...
```

### Finalized Phase (Immutable Content)

```
assets (metadata, no contents column)
    │
    ├── asset_drafts (one-to-one provenance table)
    │       └── draft_id → source draft
    │
    └── asset_chunks (associative table linking to chunks)
            └── chunks (non-versioned relational objects)
                    ├── chunk id=101: bytes 0-262143
                    ├── chunk id=102: bytes 262144-524287
                    └── chunk id=103: bytes 524288-...
```

## Schema Changes

### New Tables

```sql
-- Draft metadata (content under construction)
CREATE TABLE rev.drafts (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    asset_kind_id SMALLINT NOT NULL REFERENCES rev.asset_kinds (id),
    mime_type TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_drafts_object_id ON rev.drafts (object_id);

-- Draft chunks (append-only during draft phase)
CREATE TABLE rev.draft_chunks (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    draft_id BIGINT NOT NULL REFERENCES rev.drafts (id),
    data BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT draft_chunk_size CHECK (octet_length(data) <= 262144)  -- 256KB max
);

CREATE INDEX idx_draft_chunks_draft_id ON rev.draft_chunks (draft_id);

-- Finalized chunks (immutable, shared across asset versions)
CREATE TABLE rev.chunks (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    object_id UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    data BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT chunk_size CHECK (octet_length(data) <= 262144)  -- 256KB max
);

CREATE INDEX idx_chunks_object_id ON rev.chunks (object_id);

-- Associative table: assets → chunks (ordered by chunk.id)
CREATE TABLE rev.asset_chunks (
    asset_id BIGINT NOT NULL REFERENCES rev.assets (id),
    chunk_id BIGINT NOT NULL REFERENCES rev.chunks (id),
    PRIMARY KEY (asset_id, chunk_id)
);

CREATE INDEX idx_asset_chunks_asset_id ON rev.asset_chunks (asset_id);
CREATE INDEX idx_asset_chunks_chunk_id ON rev.asset_chunks (chunk_id);

-- One-to-one provenance table: asset version → draft
CREATE TABLE rev.asset_drafts (
    asset_id BIGINT NOT NULL REFERENCES rev.assets (id),
    draft_id BIGINT NOT NULL REFERENCES rev.drafts (id),
    PRIMARY KEY (asset_id, draft_id),
    CONSTRAINT uk_asset_drafts_asset_id UNIQUE (asset_id)
);

CREATE INDEX idx_asset_drafts_draft_id ON rev.asset_drafts (draft_id);
```

### Modifications to Existing Tables

```sql
-- Remove contents column only (no new columns added)
ALTER TABLE rev.assets
    DROP COLUMN contents;
```

## Chunk Size

**256KB (262,144 bytes)** per chunk.

| Asset Size | Chunk Count |
|------------|-------------|
| 1 MB | 4 chunks |
| 10 MB | 40 chunks |
| 50 MB | 200 chunks |
| 100 MB | 400 chunks |

## Workflow

### 1. Create Draft

```sql
INSERT INTO rev.drafts (asset_kind_id, mime_type)
VALUES ($1, $2)
RETURNING id, object_id, created_at;
```

### 2. Append Chunks to Draft (Streaming)

As content is generated (e.g., TTS audio), append chunks:

```sql
INSERT INTO rev.draft_chunks (draft_id, data)
VALUES ($1, $2)
RETURNING id, created_at;
```

Each INSERT is immediate and immutable. Chunks can be read as soon as they're written.

### 3. Finalize Draft → Asset

When content generation is complete, atomically:
1. Copy draft_chunks to chunks
2. Create asset with version
3. Link via asset_chunks
4. Record provenance (draft_id)

```sql
WITH draft_info AS (
    SELECT d.id, d.object_id, d.asset_kind_id, d.mime_type
    FROM rev.drafts d
    WHERE d.object_id = $1
),
-- Copy draft chunks to permanent chunks
new_chunks AS (
    INSERT INTO rev.chunks (data)
    SELECT dc.data
    FROM rev.draft_chunks dc
    JOIN draft_info di ON di.id = dc.draft_id
    ORDER BY dc.id
    RETURNING id, octet_length(data) as chunk_size
),
-- Calculate total content length
content_stats AS (
    SELECT
        COUNT(*) as chunk_count,
        SUM(chunk_size) as total_length
    FROM new_chunks
),
-- Create object path
new_path AS (
    INSERT INTO rev.object_paths (entity_type_id)
    SELECT et.id FROM rev.entity_types et WHERE et.key = 'asset'
    RETURNING id, object_id
),
-- Create path details
new_path_details AS (
    INSERT INTO rev.asset_path_details (path_id, asset_kind_id)
    SELECT np.id, di.asset_kind_id
    FROM new_path np, draft_info di
),
-- Create versioned object
new_version AS (
    INSERT INTO rev.versioned_objects (path_id, version_major, version_minor, version_patch)
    SELECT np.id, 0, 0, 1
    FROM new_path np
    RETURNING id, object_id, path_id
),
-- Create asset
new_asset AS (
    INSERT INTO rev.assets (versioned_object_id)
    SELECT nv.id
    FROM new_version nv
    RETURNING id, object_id, versioned_object_id
),
-- Link asset to draft (provenance)
new_asset_draft AS (
    INSERT INTO rev.asset_drafts (asset_id, draft_id)
    SELECT na.id, di.id
    FROM new_asset na, draft_info di
),
-- Link asset to chunks
new_links AS (
    INSERT INTO rev.asset_chunks (asset_id, chunk_id)
    SELECT na.id, nc.id
    FROM new_asset na
    CROSS JOIN new_chunks nc
)
SELECT
    na.id,
    na.object_id,
    nv.path_id,
    np.object_id as path_object_id,
    di.id as draft_id,
    di.object_id as draft_object_id,
    cs.total_length as content_length,
    di.mime_type,
    cs.chunk_count
FROM new_asset na
JOIN new_version nv ON nv.id = na.versioned_object_id
JOIN new_path np ON np.id = nv.path_id
CROSS JOIN draft_info di
CROSS JOIN content_stats cs;
```

## Query Patterns

### Read Chunks for Asset (Full Content)

```sql
SELECT c.id, c.data
FROM rev.asset_chunks ac
JOIN rev.chunks c ON c.id = ac.chunk_id
WHERE ac.asset_id = $1
ORDER BY c.id;
```

### Read Specific Byte Range

Given byte range `start` to `end`:

```sql
WITH asset_info AS (
    SELECT a.id, a.content_length
    FROM rev.assets a
    WHERE a.object_id = $1
),
-- Calculate which chunks contain the requested range
chunk_positions AS (
    SELECT
        c.id,
        c.data,
        SUM(octet_length(c.data)) OVER (ORDER BY c.id) - octet_length(c.data) as start_byte,
        SUM(octet_length(c.data)) OVER (ORDER BY c.id) - 1 as end_byte
    FROM rev.asset_chunks ac
    JOIN rev.chunks c ON c.id = ac.chunk_id
    JOIN asset_info ai ON ai.id = ac.asset_id
    ORDER BY c.id
)
SELECT id, data, start_byte, end_byte
FROM chunk_positions
WHERE end_byte >= $2    -- range start
  AND start_byte <= $3  -- range end
ORDER BY id;
```

Application code then extracts the exact byte range from the returned chunks.

### Read Draft Chunks (During Streaming)

```sql
SELECT dc.id, dc.data, dc.created_at
FROM rev.draft_chunks dc
JOIN rev.drafts d ON d.id = dc.draft_id
WHERE d.object_id = $1
ORDER BY dc.id;
```

## Rust Models

### Draft Models

```rust
pub struct Draft {
    pub object_id: Uuid,
    pub asset_kind: String,
    pub mime_type: String,
    pub created_at: DateTime,
}

pub struct DraftChunk {
    pub id: i64,
    pub draft_id: i64,
    pub data: Vec<u8>,
    pub created_at: DateTime,
}

pub struct InsertDraftArgs {
    pub asset_kind_keys: Vec<String>,
    pub mime_types: Vec<String>,
}

pub struct InsertDraftChunkArgs {
    pub draft_ids: Vec<i64>,
    pub data: Vec<Vec<u8>>,
}
```

### Chunk Models

```rust
pub struct Chunk {
    pub id: i64,
    pub object_id: Uuid,
    pub data: Vec<u8>,
    pub created_at: DateTime,
}
```

### Asset Models

```rust
pub struct Asset {
    pub object_id: Uuid,
    pub path_object_id: Uuid,
    pub asset_kind: String,
    pub version: Version,
    pub deprecation_level: Option<String>,
    pub created_at: DateTime,
}

pub struct AssetWithMetadata {
    pub object_id: Uuid,
    pub path_object_id: Uuid,
    pub asset_kind: String,
    pub version: Version,
    pub deprecation_level: Option<String>,
    pub draft_object_id: Option<Uuid>,
    pub content_length: i64,
    pub mime_type: String,
    pub created_at: DateTime,
}
```

## File Locations

```
dapple-data/
├── migrations/
│   └── YYYYMMDD_asset_chunks_migration.sql
├── assets/sql/
│   ├── drafts/
│   │   ├── insert_drafts.sql
│   │   ├── select_drafts_by_object_ids.sql
│   │   └── finalize_draft.sql
│   ├── draft_chunks/
│   │   ├── insert_draft_chunks.sql
│   │   └── select_draft_chunks_by_draft_id.sql
│   ├── chunks/
│   │   └── select_chunks_by_asset_id.sql
│   ├── asset_drafts/
│   │   └── insert_asset_drafts.sql
│   └── assets/
│       ├── insert_assets.sql
│       ├── select_assets_by_object_ids.sql
│       └── select_asset_content_range.sql
└── src/
    ├── model/
    │   ├── drafts.rs
    │   ├── chunks.rs
    │   └── assets.rs
    └── query/
        ├── drafts.rs
        ├── chunks.rs
        └── assets.rs
```

## Migration Strategy

1. Create new tables (`drafts`, `draft_chunks`, `chunks`, `asset_chunks`, `asset_drafts`)
2. Migrate existing assets:
   - For each existing asset with `contents`:
     - Create chunk(s) from contents
     - Link via asset_chunks
3. Drop `contents` column from assets

## Notes

### Out of Scope

- **Cost tracking**: TTS and other generation has compute cost that should be metered. This is tracked separately and not part of this plan.
- **Orphan cleanup**: A future query will archive orphaned draft_chunks from failed/abandoned drafts.

### Prohibited Operations

- `UPDATE` on `rev.chunks` — chunks are immutable
- `UPDATE` on `rev.draft_chunks` — draft chunks are append-only
- `DELETE` on `rev.chunks` while referenced by asset_chunks

### Versioning Behavior

When an asset is patched (new version):
- New chunks are created (or existing chunks reused if identical)
- New asset row with incremented version
- New asset_chunks links
- Old version remains intact with its chunk links
