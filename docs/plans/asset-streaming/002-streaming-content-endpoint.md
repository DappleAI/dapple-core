# Plan: Streaming Content Endpoint

## Overview

Add HTTP streaming endpoints for asset and draft content that support:
- Raw binary responses (not base64 JSON)
- HTTP Range requests for seeking (audio/video)
- Streaming from chunks without loading full content into memory
- Draft content access during generation

## Endpoints

### Asset Content

```
GET /api/assets/{id}/content
```

Returns finalized asset content as raw binary stream.

### Draft Content

```
GET /api/drafts/{id}/content
```

Returns draft content as raw binary stream. Can be called while draft is still being written — returns chunks available so far.

## HTTP Response Patterns

### Full Content (No Range Header)

```http
GET /api/assets/{id}/content HTTP/1.1
Accept: */*

HTTP/1.1 200 OK
Content-Type: audio/wav
Content-Length: 1440000
Accept-Ranges: bytes

[binary data streamed from chunks]
```

### Partial Content (Range Header)

```http
GET /api/assets/{id}/content HTTP/1.1
Accept: */*
Range: bytes=262144-524287

HTTP/1.1 206 Partial Content
Content-Type: audio/wav
Content-Range: bytes 262144-524287/1440000
Content-Length: 262144
Accept-Ranges: bytes

[binary data for requested range]
```

### Draft Content (May Be Incomplete)

```http
GET /api/drafts/{id}/content HTTP/1.1
Accept: */*

HTTP/1.1 200 OK
Content-Type: audio/wav
Transfer-Encoding: chunked
X-Draft-Status: in-progress
X-Chunks-Available: 3

[binary data from available chunks]
```

Note: Draft responses use `Transfer-Encoding: chunked` instead of `Content-Length` since total size is unknown until finalized.

## Handler Implementation

### Asset Content Handler

```rust
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, Response, StatusCode},
};
use futures::StreamExt;
use tokio_util::io::ReaderStream;

pub async fn asset_content_handler(
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Response<Body>, ApiError> {
    // 1. Get asset metadata (NOT content)
    let asset = select_asset_metadata_by_object_id(&state.pool, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    // 2. Parse Range header
    let range = parse_range_header(&headers, asset.content_length);

    // 3. Build response based on range
    match range {
        Some((start, end)) => {
            // Partial content response
            let chunks = select_asset_chunks_for_range(
                &state.pool,
                asset.id,
                start,
                end,
            ).await?;

            let body = stream_chunks_with_range(chunks, start, end);
            let content_length = end - start + 1;

            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, &asset.mime_type)
                .header(header::CONTENT_LENGTH, content_length)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", start, end, asset.content_length),
                )
                .body(body)
                .map_err(|_| ApiError::Internal)
        }
        None => {
            // Full content response
            let chunks = select_asset_chunks(&state.pool, asset.id).await?;
            let body = stream_chunks(chunks);

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, &asset.mime_type)
                .header(header::CONTENT_LENGTH, asset.content_length)
                .header(header::ACCEPT_RANGES, "bytes")
                .body(body)
                .map_err(|_| ApiError::Internal)
        }
    }
}
```

### Draft Content Handler

```rust
pub async fn draft_content_handler(
    Path(id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
) -> Result<Response<Body>, ApiError> {
    // 1. Get draft metadata
    let draft = select_draft_by_object_id(&state.pool, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    // 2. Get available chunks
    let chunks = select_draft_chunks(&state.pool, draft.id).await?;
    let chunk_count = chunks.len();

    // 3. Stream available chunks (Transfer-Encoding: chunked)
    let body = stream_chunks(chunks);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, &draft.mime_type)
        .header(header::TRANSFER_ENCODING, "chunked")
        .header("X-Draft-Status", "in-progress")
        .header("X-Chunks-Available", chunk_count.to_string())
        .body(body)
        .map_err(|_| ApiError::Internal)
}
```

### Chunk Streaming Helper

```rust
fn stream_chunks(chunks: Vec<ChunkData>) -> Body {
    let stream = futures::stream::iter(chunks.into_iter().map(|chunk| {
        Ok::<_, std::io::Error>(chunk.data)
    }));

    Body::from_stream(stream)
}

fn stream_chunks_with_range(
    chunks: Vec<ChunkWithPosition>,
    range_start: i64,
    range_end: i64,
) -> Body {
    let stream = futures::stream::iter(chunks.into_iter().map(move |chunk| {
        // Calculate which portion of this chunk falls within the range
        let chunk_start = chunk.start_byte;
        let chunk_end = chunk.start_byte + chunk.data.len() as i64 - 1;

        let slice_start = (range_start - chunk_start).max(0) as usize;
        let slice_end = ((range_end - chunk_start + 1) as usize).min(chunk.data.len());

        let slice = chunk.data[slice_start..slice_end].to_vec();
        Ok::<_, std::io::Error>(slice)
    }));

    Body::from_stream(stream)
}
```

## Range Header Parsing

```rust
fn parse_range_header(headers: &HeaderMap, content_length: i64) -> Option<(i64, i64)> {
    let range_header = headers.get(header::RANGE)?;
    let range_str = range_header.to_str().ok()?;

    // Parse "bytes=START-END" or "bytes=START-" or "bytes=-SUFFIX"
    if !range_str.starts_with("bytes=") {
        return None;
    }

    let range_spec = &range_str[6..];
    let parts: Vec<&str> = range_spec.split('-').collect();

    match parts.as_slice() {
        [start, ""] => {
            // "bytes=START-" -> from START to end
            let start: i64 = start.parse().ok()?;
            Some((start, content_length - 1))
        }
        ["", suffix] => {
            // "bytes=-SUFFIX" -> last SUFFIX bytes
            let suffix: i64 = suffix.parse().ok()?;
            Some((content_length - suffix, content_length - 1))
        }
        [start, end] => {
            // "bytes=START-END"
            let start: i64 = start.parse().ok()?;
            let end: i64 = end.parse().ok()?;
            Some((start, end.min(content_length - 1)))
        }
        _ => None,
    }
}
```

## SQL Queries

### Select Asset Metadata (No Content)

```sql
-- assets/select_asset_metadata_by_object_id.sql
SELECT
    a.id,
    a.object_id,
    a.content_length,
    a.mime_type,
    ak.key as asset_kind_key
FROM rev.assets a
JOIN rev.versioned_objects vo ON vo.id = a.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.asset_path_details apd ON apd.path_id = op.id
JOIN rev.asset_kinds ak ON ak.id = apd.asset_kind_id
WHERE a.object_id = $1;
```

### Select All Chunks for Asset

```sql
-- chunks/select_chunks_by_asset_id.sql
SELECT c.id, c.data
FROM rev.asset_chunks ac
JOIN rev.chunks c ON c.id = ac.chunk_id
WHERE ac.asset_id = $1
ORDER BY c.id;
```

### Select Chunks for Byte Range

```sql
-- chunks/select_chunks_for_range.sql
WITH chunk_positions AS (
    SELECT
        c.id,
        c.data,
        SUM(octet_length(c.data)) OVER (ORDER BY c.id ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) as start_byte
    FROM rev.asset_chunks ac
    JOIN rev.chunks c ON c.id = ac.chunk_id
    WHERE ac.asset_id = $1
    ORDER BY c.id
),
positioned AS (
    SELECT
        id,
        data,
        COALESCE(start_byte, 0) as start_byte,
        COALESCE(start_byte, 0) + octet_length(data) - 1 as end_byte
    FROM chunk_positions
)
SELECT id, data, start_byte, end_byte
FROM positioned
WHERE end_byte >= $2    -- range_start
  AND start_byte <= $3  -- range_end
ORDER BY id;
```

### Select Draft Chunks

```sql
-- draft_chunks/select_draft_chunks_by_draft_id.sql
SELECT dc.id, dc.data, dc.created_at
FROM rev.draft_chunks dc
WHERE dc.draft_id = $1
ORDER BY dc.id;
```

## Route Registration

```rust
// route/assets.rs
pub fn asset_routes() -> ApiRouter<AppState> {
    ApiRouter::new()
        .api_route("/assets", post(asset_post_handler))
        .api_route("/assets", get(asset_index_handler))
        .api_route("/assets/:id", get(asset_get_handler))
        .route("/assets/:id/content", get(asset_content_handler))  // Raw binary, not OpenAPI
        .api_route("/assets", patch(asset_patch_handler))
        .api_route("/assets", delete(asset_delete_handler))
}

// route/drafts.rs
pub fn draft_routes() -> ApiRouter<AppState> {
    ApiRouter::new()
        .api_route("/drafts", post(draft_post_handler))
        .api_route("/drafts", get(draft_index_handler))
        .api_route("/drafts/:id", get(draft_get_handler))
        .route("/drafts/:id/content", get(draft_content_handler))  // Raw binary, not OpenAPI
        .api_route("/drafts/:id/chunks", post(draft_chunk_post_handler))
        .api_route("/drafts/:id/finalize", post(draft_finalize_handler))
}
```

Note: `/content` routes use `.route()` not `.api_route()` since binary responses don't fit OpenAPI JSON schema.

## Browser Compatibility

For HTML5 `<audio>` and `<video>` elements:

```html
<audio src="/api/assets/{id}/content" controls />
<video src="/api/assets/{id}/content" controls />
```

These elements will:
1. Make initial request to get metadata
2. Use Range requests for seeking
3. Buffer chunks as needed

The endpoint must support:
- `Accept-Ranges: bytes` header (indicates range support)
- Proper `Content-Type` for media detection
- `Content-Length` for progress indication
- `206 Partial Content` for range requests

## Error Responses

| Condition | Status | Response |
|-----------|--------|----------|
| Asset not found | 404 | `{"error": "not_found"}` |
| Invalid range | 416 | `Range Not Satisfiable` |
| Draft not found | 404 | `{"error": "not_found"}` |
| Internal error | 500 | `{"error": "internal"}` |

## Notes

### Out of Scope

- **Cost tracking**: Bandwidth/streaming costs should be metered separately.

### Performance Considerations

- Chunks are streamed directly from database — not loaded entirely into memory
- Range queries only fetch relevant chunks
- Consider adding caching headers (`Cache-Control`, `ETag`) for finalized assets in future
