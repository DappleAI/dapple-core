# Plan: TTS Normalization via Prompt System

## Objective

Normalize TTS to work through the existing `prompt send` command rather than a separate `synth` command. TTS becomes just another prompt type routed by `maker_key`.

## Before/After

**Before (current):**
```nushell
# Separate commands with different patterns
$text | prompt send "translate"           # LLM prompt
$text | summarize --prompt "normalizer"   # T5 (separate command)
$text | synth --voice "A calm speaker"    # TTS (separate command, literal voice)
```

**After (unified):**
```nushell
# All go through prompt send, routed by maker_key
$text | prompt send "translate"           # maker_key: anthropic
$text | prompt send "activity-normalizer" # maker_key: t5
$text | prompt send "narrator-voice"      # maker_key: tts
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  Nushell Script                                                  │
│  $text | prompt send "narrator-voice"                           │
└─────────────────────────┬───────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│  prompt send command                                             │
│  - Looks up prompt_object_id from DAPPLE_METADATA.prompt_mappings│
│  - Calls conduit /prompt endpoint                               │
└─────────────────────────┬───────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│  Conduit /prompt endpoint                                        │
│  - Resolves prompt → engine_config                              │
│  - Routes based on maker_key:                                   │
│    ┌─────────────────────────────────────────────────────────┐  │
│    │ anthropic/mistral → LlmService                          │  │
│    │ t5/summarize      → SummarizationService                │  │
│    │ tts/parler        → TtsService                          │  │
│    └─────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

## Changes Required

### 1. Add TTS Routing in Conduit

**File:** `crates/dapple-prompt-conduit/src/handler/prompt.rs`

The prompt handler already routes based on `maker_key`. Add TTS routing:

```rust
// In handle_prompt or the routing logic
match engine_config.maker_key.as_str() {
    "anthropic" => llm_service.execute(...).await,
    "mistral" | "local" => llm_service.execute(...).await,
    "t5" | "summarize" => summarization_service.summarize(...).await,
    "tts" | "parler" => tts_service.synthesize(...).await,  // NEW
    _ => Err(ConduitError::UnsupportedEngine(...)),
}
```

### 2. TTS Prompt Configuration

For TTS prompts, the `description` field contains the Parler-TTS voice description:

```sql
-- Create TTS prompt
INSERT INTO rev.prompt_paths (object_id) VALUES (gen_random_uuid());

INSERT INTO rev.prompts (path_id, name, description, template)
VALUES (
    $path_id,
    'narrator-voice',
    'A calm female speaker with a British accent, speaking clearly and slowly',
    ''  -- No template needed for TTS
);

-- Set engine_config with maker_key = 'tts'
INSERT INTO rev.engine_configs (...)
VALUES (..., 'tts', ...);

-- Link prompt to engine_config via appropriate overlay or default
```

### 3. TTS Response Handling

TTS returns an asset, not text. The prompt handler needs to handle this:

```rust
// For TTS maker_key
let tts_result = tts_service.synthesize(&input_text, voice_description).await?;

// Create asset via draft workflow
let draft = drafts::insert_drafts(&pool, ...).await?;
for chunk in tts_result.audio_bytes.chunks(CHUNK_SIZE) {
    draft_chunks::insert_draft_chunks(&pool, ...).await?;
}
let asset = drafts::finalize_draft(&pool, draft.object_id).await?;

// Return asset reference in response
// Format TBD - could be JSON with asset_object_id, or special marker
```

**Response format options:**

Option A: JSON response
```json
{
  "type": "asset",
  "asset_object_id": "550e8400-...",
  "duration_seconds": 2.5,
  "content_length": 120000
}
```

Option B: Special marker in text response
```
[asset:550e8400-e29b-41d4-a716-446655440000]
```

Option C: Structured response type (requires prompt handler changes)
```rust
enum PromptResponse {
    Text(String),
    Asset { object_id: Uuid, metadata: AssetMetadata },
}
```

### 4. Refactor Synth Command as Alias

`synth` becomes a thin wrapper around `prompt send`:

```rust
// synth.rs - refactored
fn run(&self, ...) -> Result<PipelineData, ShellError> {
    let prompt_name: String = call.get_flag(..., "prompt")?.ok_or(...)?;

    // Delegate to prompt send
    let response = execute_prompt(&prompt_name, &metadata, span)?;

    // Parse asset response (JSON or structured)
    let asset_info: AssetResponse = serde_json::from_str(&response)?;

    // Return structured record for convenience
    let record = record! {
        "asset_object_id" => Value::string(asset_info.asset_object_id.to_string(), span),
        "draft_object_id" => Value::string(asset_info.draft_object_id.to_string(), span),
        "duration_seconds" => Value::float(asset_info.duration_seconds as f64, span),
        "sample_rate" => Value::int(asset_info.sample_rate as i64, span),
        "content_length" => Value::int(asset_info.content_length, span),
        "text" => Value::string(asset_info.text, span),
    };

    Ok(PipelineData::Value(Value::record(record, span), None))
}
```

The synth command provides:
- `--prompt` flag (required)
- Structured record output (not raw JSON)
- Clearer error messages for TTS-specific failures

### 5. Refactor Summarize Command as Alias

Same pattern — summarize delegates to `prompt send`:

```rust
// summarize.rs - refactored
fn run(&self, ...) -> Result<PipelineData, ShellError> {
    let prompt_name: String = call.get_flag(..., "prompt")?.ok_or(...)?;

    // Delegate to prompt send
    let response = execute_prompt(&prompt_name, &metadata, span)?;

    // Summarize returns text, so just pass through
    Ok(PipelineData::Value(Value::string(response, span), None))
}
```

The summarize command provides:
- `--prompt` flag (required)
- `--max-length`, `--min-length` flags (if still needed, passed via context)
- Familiar interface for text summarization tasks

## Prompt Setup for TTS

### Create Voice Prompt

```sql
-- 1. Create path
INSERT INTO rev.prompt_paths (object_id)
VALUES (gen_random_uuid())
RETURNING id, object_id;

-- 2. Create prompt (description = voice description for Parler-TTS)
INSERT INTO rev.prompts (path_id, name, description, template, version_major, version_minor, version_patch)
VALUES (
    $path_id,
    'narrator-voice',
    'A calm female speaker with a British accent, speaking clearly and slowly',
    '',  -- No template
    0, 0, 1
);

-- 3. Create/use engine_config with maker_key = 'tts'
-- (May need new engine_config or reuse existing TTS config)
```

### Bind to Task

```sql
INSERT INTO rev.task_prompts (task_id, prompt_id)
VALUES ($task_id, $prompt_id);
```

## Migration Notes

### Breaking Changes

1. `synth --voice` removed — must use `synth --prompt` with a TTS prompt
2. Both commands now require prompts to be bound via task_prompts

### Migration Path

```nushell
# Step 1: Create TTS prompts for existing voice configs
# (voice description goes in prompt.description field)

# Step 2: Update scripts
# Old:
$text | synth --voice "A calm speaker"
# New:
$text | synth --prompt "narrator-voice"

# Both work:
$text | synth --prompt "narrator-voice"     # Returns structured record
$text | prompt send "narrator-voice"         # Returns raw JSON (or text)
```

### Direct prompt send Usage

Users can also call `prompt send` directly if they don't need the structured output:

```nushell
# These are equivalent, different output formats:
$text | synth --prompt "narrator-voice"
# → { asset_object_id: "...", duration_seconds: 2.5, ... }

$text | prompt send "narrator-voice"
# → {"type":"asset","asset_object_id":"...","duration_seconds":2.5,...}
```

## Open Questions

1. **Response format for assets**: How should TTS results (assets) be returned through `prompt send`? JSON? Special marker? New response type?

2. **Engine config for TTS**: Do we need a dedicated TTS engine_config, or can it be inferred from maker_key alone?

3. **Input handling**: TTS takes plain text. Should the prompt template be used, or should it pass input directly to TTS?

4. **Default voice**: If prompt description is empty, use TTS_DEFAULT_VOICE from env?

## File Locations

```
dapple-prompt-conduit/
├── src/
│   ├── handler/
│   │   └── prompt.rs          # MODIFIED: add TTS routing
│   └── service/
│       └── tts.rs             # Existing TTS service

dapple-prompt-flow/
├── src/
│   └── commands/
│       ├── synth.rs           # MODIFIED: delegate to prompt, parse response
│       └── summarize.rs       # MODIFIED: delegate to prompt (future)
```

## Implementation Order

1. Add `tts` maker_key routing in conduit prompt handler
2. Define TTS response format (JSON with asset metadata)
3. Handle asset creation in conduit (draft → chunks → finalize)
4. Refactor `synth` command to delegate to prompt system
5. Test end-to-end: script → prompt send → conduit → TTS → asset
6. (Future) Apply same pattern to `summarize`
