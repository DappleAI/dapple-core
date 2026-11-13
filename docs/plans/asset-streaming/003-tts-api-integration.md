# Plan: TTS API Integration

## Overview

Expose TTS (Text-to-Speech) functionality through dapple-prompt-api as the single entry point for all external consumers (dapple-suite packages). Conduit remains physically isolated — internal only.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  External Consumers (dapple-suite: demo-studio, etc.)           │
└─────────────────────────┬───────────────────────────────────────┘
                          │ HTTP (only allowed connection)
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│  dapple-prompt-api                                              │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │  POST /api/synthesize                                      │ │
│  │  - Creates draft                                           │ │
│  │  - Calls conduit internally                                │ │
│  │  - Streams chunks to draft_chunks                          │ │
│  │  - Finalizes to asset                                      │ │
│  │  - Returns asset_object_id                                 │ │
│  └────────────────────────────────────────────────────────────┘ │
│                            │                                    │
│                            │ internal (physically isolated)     │
│                            ▼                                    │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │  conduit (TTS service)                                     │ │
│  │  - Parler-TTS inference                                    │ │
│  │  - Returns audio bytes                                     │ │
│  └────────────────────────────────────────────────────────────┘ │
│                            │                                    │
│                            │ via dapple-data library            │
│                            ▼                                    │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │  PostgreSQL (drafts, draft_chunks, assets, asset_chunks)   │ │
│  └────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

## API Endpoint

### POST /api/synthesize

Synthesize text to speech, store as asset, return reference.

**Request:**

```json
{
  "text": "Hello, welcome to the demo.",
  "voice": "A calm female speaker with a British accent",
  "mime_type": "audio/wav"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `text` | string | yes | Text to synthesize |
| `voice` | string | no | Voice description for Parler-TTS |
| `mime_type` | string | no | Output format (default: `audio/wav`) |

**Response:**

```json
{
  "asset_object_id": "550e8400-e29b-41d4-a716-446655440000",
  "draft_object_id": "660e8400-e29b-41d4-a716-446655440001",
  "duration_seconds": 2.5,
  "sample_rate": 24000,
  "content_length": 120000,
  "text": "Hello, welcome to the demo."
}
```

| Field | Type | Description |
|-------|------|-------------|
| `asset_object_id` | UUID | Finalized asset reference |
| `draft_object_id` | UUID | Source draft (provenance) |
| `duration_seconds` | float | Audio duration |
| `sample_rate` | int | Audio sample rate (Hz) |
| `content_length` | int | Total bytes |
| `text` | string | Text that was synthesized |

## Handler Implementation

```rust
// handler/synthesize.rs

use crate::error::ApiError;
use crate::schema::synthesize::{SynthesizeRequest, SynthesizeResponse};
use axum::{extract::State, Json};
use dapple_data::model::drafts::InsertDraftArgs;
use dapple_data::query::{drafts, draft_chunks};
use std::sync::Arc;

/// Chunk size for draft chunks (256KB)
const CHUNK_SIZE: usize = 262_144;

pub async fn synthesize_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SynthesizeRequest>,
) -> Result<Json<SynthesizeResponse>, ApiError> {
    // 1. Validate input
    if req.text.is_empty() {
        return Err(ApiError::BadRequest("Text cannot be empty".into()));
    }

    let mime_type = req.mime_type.unwrap_or_else(|| "audio/wav".to_string());

    // 2. Create draft
    let draft = drafts::insert_drafts(
        &state.pool,
        InsertDraftArgs {
            asset_kind_keys: vec!["audio".to_string()],
            mime_types: vec![mime_type.clone()],
        },
    )
    .await?
    .into_iter()
    .next()
    .ok_or(ApiError::Internal)?;

    // 3. Call conduit for TTS (internal)
    let tts_result = state
        .conduit_client
        .synthesize(&req.text, req.voice.as_deref())
        .await
        .map_err(|e| ApiError::ServiceError(format!("TTS failed: {}", e)))?;

    // 4. Write audio bytes as chunks
    let audio_bytes = tts_result.audio_bytes;
    for chunk_data in audio_bytes.chunks(CHUNK_SIZE) {
        draft_chunks::insert_draft_chunks(
            &state.pool,
            dapple_data::model::draft_chunks::InsertDraftChunkArgs {
                draft_ids: vec![draft.id],
                data: vec![chunk_data.to_vec()],
            },
        )
        .await?;
    }

    // 5. Finalize draft to asset
    let asset = drafts::finalize_draft(&state.pool, draft.object_id).await?;

    // 6. Return response
    Ok(Json(SynthesizeResponse {
        asset_object_id: asset.object_id,
        draft_object_id: draft.object_id,
        duration_seconds: tts_result.duration_seconds,
        sample_rate: tts_result.sample_rate,
        content_length: asset.content_length,
        text: tts_result.text,
    }))
}
```

## Schema

```rust
// schema/synthesize.rs

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SynthesizeRequest {
    /// The text to synthesize into speech
    pub text: String,

    /// Voice description for Parler-TTS
    /// e.g., "A calm female speaker with a British accent"
    #[serde(default)]
    pub voice: Option<String>,

    /// Output MIME type (default: audio/wav)
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SynthesizeResponse {
    /// The object_id of the created Asset
    pub asset_object_id: Uuid,

    /// The object_id of the source Draft (provenance)
    pub draft_object_id: Uuid,

    /// Duration of the generated audio in seconds
    pub duration_seconds: f32,

    /// Sample rate of the audio (Hz)
    pub sample_rate: u32,

    /// Total content length in bytes
    pub content_length: i64,

    /// The text that was synthesized
    pub text: String,
}
```

## Conduit Client (Internal)

The API uses an internal client to call conduit. This client is NOT exposed externally.

```rust
// service/conduit_client.rs

use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct ConduitClient {
    client: Client,
    base_url: String,
}

#[derive(Debug, Serialize)]
struct ConduitSynthesizeRequest {
    text: String,
    voice: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ConduitSynthesizeResponse {
    pub audio_bytes: Vec<u8>,
    pub duration_seconds: f32,
    pub sample_rate: u32,
    pub text: String,
}

impl ConduitClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.to_string(),
        }
    }

    pub async fn synthesize(
        &self,
        text: &str,
        voice: Option<&str>,
    ) -> Result<ConduitSynthesizeResponse, ConduitError> {
        let resp = self
            .client
            .post(format!("{}/synthesize", self.base_url))
            .json(&ConduitSynthesizeRequest {
                text: text.to_string(),
                voice: voice.map(String::from),
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(resp)
    }
}
```

## Conduit Changes

Conduit's `/synthesize` endpoint needs modification:
- Remove direct asset creation (no longer writes to DB)
- Return raw audio bytes in response
- Let dapple-prompt-api handle storage

**Before (current):**
```rust
// conduit creates asset directly
let assets = insert_assets(&state.pool, ...).await?;
Ok(Json(SynthesizeResponse { asset_object_id: ... }))
```

**After:**
```rust
// conduit returns bytes, caller handles storage
Ok(Json(SynthesizeResponse {
    audio_bytes: result.audio_bytes,  // Raw bytes
    duration_seconds: result.duration_seconds,
    sample_rate: result.sample_rate,
    text: result.text,
}))
```

## Route Registration

```rust
// route/synthesize.rs

use aide::axum::{routing::post, ApiRouter};
use crate::handler::synthesize::synthesize_handler;

pub fn synthesize_routes() -> ApiRouter<AppState> {
    ApiRouter::new()
        .api_route("/synthesize", post(synthesize_handler))
}

// lib.rs - add to router
app.merge(synthesize_routes())
```

## TTS in Workflow Context

TTS is handled as a regular prompt-like operation. The asset reference is recorded in the workflow context:

1. **Task execution** calls `/api/synthesize`
2. **Response** includes `asset_object_id`
3. **Workflow context** stores the asset reference
4. **Subsequent tasks** can access the audio via the stored reference
5. **Audit trail** captures:
   - Which task triggered TTS
   - Input text
   - Resulting asset_object_id
   - Draft provenance (draft_object_id)

This integrates naturally with the existing Journey/Playbook/Task model without special TTS-specific tracking.

## File Locations

```
dapple-prompt-api/
├── src/
│   ├── handler/
│   │   └── synthesize.rs       # TTS handler
│   ├── route/
│   │   └── synthesize.rs       # Route registration
│   ├── schema/
│   │   └── synthesize.rs       # Request/response types
│   └── service/
│       └── conduit_client.rs   # Internal conduit client

dapple-prompt-conduit/
├── src/
│   └── handler/
│       └── synthesize.rs       # Modified to return bytes
```

## Environment Configuration

```bash
# dapple-prompt-api
CONDUIT_URL=http://conduit:8081  # Internal URL, not exposed

# dapple-prompt-conduit (unchanged)
TTS_ENABLED=true
TTS_DEFAULT_VOICE="A calm speaker"
```

## Notes

### Out of Scope

- **Cost tracking**: TTS has compute cost that should be metered. This is tracked in a separate system and not part of this plan.

### Security

- Conduit is physically isolated from external networks
- Only dapple-prompt-api can reach conduit
- All external access goes through authenticated API endpoints

### Future Considerations

- Streaming TTS (generate and stream chunks as they're produced)
- Voice cloning / custom voices
- Multiple TTS backends (not just Parler-TTS)
