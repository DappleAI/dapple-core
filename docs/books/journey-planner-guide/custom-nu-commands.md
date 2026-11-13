# Custom Nushell Commands

> **Audience:** :wrench: Prompt Engineers · :robot: Agents
>
> *Product readers can skip this chapter.*

The dapple-prompt-flow runtime provides custom Nushell commands that extend the standard library with AI-powered operations. These commands communicate with the conduit service to perform machine learning tasks.

---

## Available Commands

| Command | Purpose | Backend |
|---------|---------|---------|
| `madlib` | Fill `[MASK]` tokens with contextual predictions | ModernBERT MLM |
| `summarize` | Compress text into concise summaries | T5 (SAMSum) |
| `json-schema` | Infer JSON Schema from data | Local (no backend) |

---

## Environment Setup

All conduit-backed commands use these environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `CONDUIT_URL` | `http://localhost:8081` | Conduit service URL |
| `DAPPLE_METADATA` | (none) | JSON with `conduit_url` override |

The runtime sets `DAPPLE_METADATA` automatically when executing Tasks.

---

## madlib — Masked Language Model Fill

Fill `[MASK]` tokens using ModernBERT's masked language modeling capability. The model predicts contextually appropriate words for each mask position.

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
#     {
#       position: 1,
#       candidates: [
#         { token: "weather", score: 0.82 },
#         { token: "food", score: 0.11 },
#         { token: "view", score: 0.04 }
#       ]
#     },
#     {
#       position: 4,
#       candidates: [
#         { token: "nice", score: 0.65 },
#         { token: "good", score: 0.22 },
#         { token: "cold", score: 0.08 }
#       ]
#     }
#   ]
# }
```

### Use Cases

1. **Dynamic template generation:**
   ```nushell
   let template = "Dear [MASK], Thank you for your [MASK]."
   $template | madlib
   ```

2. **Context-aware word selection:**
   ```nushell
   let context = $"Based on the user's request to ($action), the next step is to [MASK]."
   $context | madlib
   ```

3. **Generate variations:**
   ```nushell
   "The [MASK] solution would be" | madlib --top-k 5 --verbose | get predictions.0.candidates
   ```

### Requirements

Requires the conduit service with MLM enabled:
```bash
MLM_ENABLED=true cargo run -p dapple-prompt-conduit
```

---

## summarize — Text Summarization

Compress text into concise summaries using a T5 model fine-tuned on the SAMSum dialogue dataset. Optimized for conversational content but works on any text.

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

The command accepts multiple input types:

| Input Type | Behavior |
|------------|----------|
| String | Used directly |
| List | Elements joined with newlines |
| Record | Fields formatted as "key: value" lines |

### Examples

**Basic summarization:**
```nushell
"Alice: Hi! How are you?\nBob: Great, thanks for asking!\nAlice: Want to grab lunch?\nBob: Sure, noon works." | summarize
# Output: "Alice and Bob agree to meet for lunch at noon."
```

**With length constraints:**
```nushell
open transcript.txt | summarize --max-length 50
```

**Summarize debrief responses:**
```nushell
$debrief.responses | to text | summarize
```

**Summarize a record:**
```nushell
{ speaker: "Alice", content: "Hello there!", mood: "friendly" } | summarize
# Converts to: "speaker: Alice\ncontent: Hello there!\nmood: friendly"
# Then summarizes
```

### Use Cases

1. **Compress user input for LLM context:**
   ```nushell
   let doc = (open $asset_path)
   let compressed = ($doc | summarize --max-length 200)
   { context: $compressed } | prompt send "analyze"
   ```

2. **Summarize conversation history:**
   ```nushell
   let history = ($journey.messages | each { |m| $"($m.role): ($m.content)" } | str join "\n")
   let summary = ($history | summarize)
   ```

3. **Create debrief summaries:**
   ```nushell
   let summary = ($debrief.responses | to text | summarize)
   $summary | save $"($env.OUT_DIR)/debrief_summary.txt"
   ```

### Requirements

Requires the conduit service with summarization enabled:
```bash
SUMMARIZATION_ENABLED=true cargo run -p dapple-prompt-conduit
```

---

## json-schema — Schema Inference

Infer a JSON Schema (draft-07) from any Nushell value. This command runs locally without requiring the conduit service.

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
| `list` | `array` (items inferred from first element) |
| `record` | `object` (properties inferred recursively) |
| `date` | `string` (ISO format) |
| `duration` | `integer` (nanoseconds) |
| `filesize` | `integer` (bytes) |
| `binary` | `string` (base64 encoded) |

### Examples

**Simple record:**
```nushell
{ name: "test", age: 30 } | json-schema
# Output:
# {
#   type: "object",
#   properties: {
#     name: { type: "string" },
#     age: { type: "integer" }
#   }
# }
```

**With title and required:**
```nushell
{ id: 1, active: true } | json-schema --title "User" --required-all
# Output:
# {
#   title: "User",
#   type: "object",
#   properties: {
#     id: { type: "integer" },
#     active: { type: "boolean" }
#   },
#   required: ["active", "id"]
# }
```

**Array schema:**
```nushell
[1, 2, 3] | json-schema
# Output:
# {
#   type: "array",
#   items: { type: "integer" }
# }
```

**Nested objects:**
```nushell
{
    user: {
        name: "Alice",
        roles: ["admin", "user"]
    }
} | json-schema
# Output:
# {
#   type: "object",
#   properties: {
#     user: {
#       type: "object",
#       properties: {
#         name: { type: "string" },
#         roles: {
#           type: "array",
#           items: { type: "string" }
#         }
#       }
#     }
#   }
# }
```

### Use Cases

1. **Document output formats:**
   ```nushell
   let sample_output = { status: "ok", count: 42 }
   $sample_output | json-schema --title "TaskOutput" | to json | save schema.json
   ```

2. **Validate structure consistency:**
   ```nushell
   let expected_schema = (open expected.json)
   let actual_schema = ($data | json-schema)
   if $expected_schema != $actual_schema {
       error make { msg: "Schema mismatch" }
   }
   ```

3. **Generate API documentation:**
   ```nushell
   let request_schema = ($sample_request | json-schema --title "Request" --required-all)
   let response_schema = ($sample_response | json-schema --title "Response")
   ```

---

## Error Handling

All conduit-backed commands provide helpful error messages when the service is unavailable:

```nushell
"test [MASK]" | madlib
# Error: Madlib failed
# Check that conduit is running and MLM is enabled (MLM_ENABLED=true)
```

```nushell
$text | summarize
# Error: Summarization failed
# Check that conduit is running and summarization is enabled (SUMMARIZATION_ENABLED=true)
```

**Best practice:** Check conduit availability at the start of your Task:

```nushell
def main [] {
    # Verify conduit is reachable
    try {
        "test" | summarize --max-length 10
    } catch {
        { message: "Conduit not available", code: 503 } | to json | save $"($env.OUT_DIR)/error.json"
        exit 1
    }

    # Continue with main logic...
}
```

---

## Command Reference Table

| Command | Input | Output | Conduit Required |
|---------|-------|--------|------------------|
| `madlib` | String with `[MASK]` | String or Record | Yes (MLM) |
| `summarize` | String, List, Record | String | Yes (Summarization) |
| `json-schema` | Any | Record (schema) | No |
