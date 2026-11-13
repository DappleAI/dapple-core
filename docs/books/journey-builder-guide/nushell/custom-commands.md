# Custom Commands Reference

> **Audience:** Backend Developers · Prompt Engineers

The dapple-prompt-flow runtime provides custom Nushell commands that extend the standard library with AI-powered operations.

## Available Commands

| Command | Purpose | Backend |
|---------|---------|---------|
| `madlib` | Fill `[MASK]` tokens with contextual predictions | ModernBERT MLM |
| `summarize` | Compress text into concise summaries | T5 (SAMSum) |
| `json-schema` | Infer JSON Schema from data | Local (no backend) |
| `prompt` | Communicate with LLM backends | Conduit service |

## Environment Setup

All conduit-backed commands use these environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `CONDUIT_URL` | `http://localhost:8081` | Conduit service URL |
| `DAPPLE_METADATA` | (none) | JSON with `conduit_url` override |

The runtime sets `DAPPLE_METADATA` automatically when executing Tasks.

## madlib - Masked Language Model Fill

Fill `[MASK]` tokens using ModernBERT's masked language modeling capability.

### Signature

```nushell
"text with [MASK] tokens" | madlib [--top-k <int>] [--verbose]
```

### Flags

| Flag | Short | Description |
|------|-------|-------------|
| `--top-k` | `-k` | Number of predictions per mask (default: 1) |
| `--verbose` | `-v` | Return full predictions record instead of filled text |

### Examples

**Basic fill:**
```nushell
"The capital of France is [MASK]." | madlib
# Output: "The capital of France is Paris."
```

**Multiple masks:**
```nushell
"I need to [MASK] the [MASK] before tomorrow" | madlib
# Output: "I need to finish the report before tomorrow"
```

**Verbose output with top-k:**
```nushell
"The [MASK] is very [MASK]" | madlib --top-k 3 --verbose
# Output:
# {
#   filled_text: "The weather is very nice",
#   predictions: [
#     { position: 1, candidates: [...] },
#     { position: 4, candidates: [...] }
#   ]
# }
```

### Requirements

```bash
MLM_ENABLED=true cargo run -p dapple-prompt-conduit
```

## summarize - Text Summarization

Compress text into concise summaries using T5 model fine-tuned on SAMSum dialogue dataset.

### Signature

```nushell
$text | summarize [--max-length <int>] [--min-length <int>]
```

### Flags

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--max-length` | `-m` | 150 | Maximum summary length in tokens |
| `--min-length` | `-n` | 20 | Minimum summary length in tokens |

### Input Types

| Input Type | Behavior |
|------------|----------|
| String | Used directly |
| List | Elements joined with newlines |
| Record | Fields formatted as "key: value" lines |

### Examples

**Basic summarization:**
```nushell
"Alice: Hi! How are you?\nBob: Great!\nAlice: Want to grab lunch?" | summarize
# Output: "Alice and Bob agree to meet for lunch."
```

**With length constraints:**
```nushell
open transcript.txt | summarize --max-length 50
```

### Requirements

```bash
SUMMARIZATION_ENABLED=true cargo run -p dapple-prompt-conduit
```

## json-schema - Schema Inference

Infer a JSON Schema (draft-07) from any Nushell value. Runs locally without conduit.

### Signature

```nushell
$data | json-schema [--title <string>] [--required-all]
```

### Flags

| Flag | Short | Description |
|------|-------|-------------|
| `--title` | `-t` | Add a title field to the root schema |
| `--required-all` | `-r` | Mark all object properties as required |

### Type Mappings

| Nushell Type | JSON Schema Type |
|--------------|------------------|
| `nothing` | `null` |
| `bool` | `boolean` |
| `int` | `integer` |
| `float` | `number` |
| `string` | `string` |
| `list` | `array` |
| `record` | `object` |

### Examples

```nushell
{ name: "test", age: 30 } | json-schema --title "User" --required-all
# Output:
# {
#   title: "User",
#   type: "object",
#   properties: {
#     name: { type: "string" },
#     age: { type: "integer" }
#   },
#   required: ["age", "name"]
# }
```

## prompt - LLM Communication

Send prompts to LLM backends via the conduit service.

### Signature

```nushell
$data | prompt send <prompt_id>
$data | prompt retry
$data | prompt reset
```

### Examples

```nushell
# Send prompt
let response = ($input | prompt send "resume-data-extractor.v1")

# Retry last prompt
let response = ($input | prompt retry)

# Reset conversation context
$input | prompt reset
```

## Error Handling

All conduit-backed commands provide helpful error messages:

```nushell
"test [MASK]" | madlib
# Error: Madlib failed
# Check that conduit is running and MLM is enabled
```

**Best practice:** Check conduit availability at Task start:

```nushell
def main [] {
    try {
        "test" | summarize --max-length 10
    } catch {
        { message: "Conduit not available", code: 503 } | to json | save $"($env.OUT_DIR)/error.json"
        exit 1
    }
    # Continue with main logic...
}
```

## Command Reference Table

| Command | Input | Output | Conduit Required |
|---------|-------|--------|------------------|
| `madlib` | String with `[MASK]` | String or Record | Yes (MLM) |
| `summarize` | String, List, Record | String | Yes (Summarization) |
| `json-schema` | Any | Record (schema) | No |
| `prompt` | Any | String | Yes (LLM) |
