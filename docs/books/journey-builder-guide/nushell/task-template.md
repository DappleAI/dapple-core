# Task Template & Conventions

> **Audience:** Backend Developers · Prompt Engineers

Recommended conventions for scripts that become Tasks.

## Task Contract

All Tasks must return a record with this structure:

```nushell
{
    payload: <any>,      # Your output data (required)
    signals: { }         # Control signals (optional)
}
```

## 1. Inputs

- Accept inputs via environment variables or stdin as JSON
- Document expected inputs in a header comment

```nushell
# TASK: collect-personal-data
# Inputs: expects $env.OUT_DIR, accepts JSON on stdin with keys: name, email
```

**Input methods:**

```nushell
# Method 1: Pipeline input (recommended)
$in | each { |item| ... }

# Method 2: Bind the pipeline once, then reference the binding
# (required when the input is read more than once)
let input = $in

# Method 3: Previous artifact file
let prev = open $env.IN_ARTIFACT
let data = open $prev.path
```

## 2. Outputs

- Write artifacts to `$env.OUT_DIR`
- Emit `artifact.json` with artifact path and metadata

```nushell
# Write artifact metadata
{
    path: $"($env.OUT_DIR)/resume.pdf"
    type: "application/pdf"
} | save --force $"($env.OUT_DIR)/artifact.json"
```

## 3. Exit Codes and Errors

- Return contract-compliant output on success
- On failure, write `error.json` to OUT_DIR

```nushell
# On error
{
    message: "Failed to parse input JSON"
    code: 400
} | save --force $"($env.OUT_DIR)/error.json"

# Still return valid contract
{ payload: null, signals: { error: true } }
```

## 4. Idempotency

Tasks should be safe to re-run:
- Overwrite existing artifacts deterministically
- Or write to versioned filenames

## 5. Logging

Write structured logs to `$env.OUT_DIR/log.jsonl`:

```nushell
{
    timestamp: (date now | format date "%Y-%m-%dT%H:%M:%S")
    level: "info"
    message: "Processing started"
} | to json --raw | save --append $"($env.OUT_DIR)/log.jsonl"
```

## Task Skeleton

```nushell
#!/usr/bin/env nu

# TASK: collect-personal-data
# Inputs: expects $env.OUT_DIR, accepts JSON on stdin
# Outputs: writes input.json and artifact.json to OUT_DIR

def main [] {
    let out_dir = $env.OUT_DIR? | default "./out"
    mkdir $out_dir

    # Read input from pipeline
    let input = $in

    # Validate required fields
    if ($input.name? | is-empty) {
        {
            message: "Missing required field: name"
            code: 400
        } | save --force $"($out_dir)/error.json"

        return { payload: null, signals: { error: true } }
    }

    # Process and save artifact
    $input | save --force $"($out_dir)/input.json"

    # Log success
    {
        timestamp: (date now | format date "%Y-%m-%dT%H:%M:%S")
        level: "info"
        message: $"Collected data for ($input.name)"
    } | to json --raw | save --append $"($out_dir)/log.jsonl"

    # Write artifact metadata
    {
        path: $"($out_dir)/input.json"
        type: "application/json"
        created_at: (date now | format date "%Y-%m-%dT%H:%M:%S")
    } | save --force $"($out_dir)/artifact.json"

    # Return contract-compliant output
    {
        payload: $input,
        signals: { status: "complete" }
    }
}
```

## Environment Variables

The runtime provides these environment variables:

| Variable | Description | Example |
|----------|-------------|---------|
| `OUT_DIR` | Where to write artifacts and logs | `/tmp/playbook/.../out` |
| `IN_ARTIFACT` | Path to previous Task's artifact | `/tmp/.../artifact.json` |
| `PLAYBOOK_ID` | Current Playbook identifier | `build-resume-playbook-123` |
| `STAGE_ID` | Current Stage identifier | `stage-1` |
| `STEP_ID` | Current Step identifier | `build-resume-step` |
| `TASK_ID` | Current Task identifier | `format-resume` |
| `DAPPLE_METADATA` | Runtime metadata | `{"conduit_url":"..."}` |

## Notes

- Prefer simple, focused scripts
- Keep external dependencies minimal
- Document required binaries in header
- **Privacy:** Mask PII when storing logs
