# Debugging

> **Audience:** Backend Developers · Platform Engineers

Debugging techniques for Tasks, Playbooks, and the runtime.

## Preserving Logs

Write both human-readable and machine-parseable logs:

```nushell
# Human-friendly to stdout
print $"Processing ($input.name)..."

# Machine-friendly to log.jsonl
{
    timestamp: (date now | format date "%Y-%m-%dT%H:%M:%S")
    level: "debug"
    task: "format-resume"
    message: "Starting markdown generation"
    input_size: ($input | to json | str length)
} | to json --raw | save --append $"($env.OUT_DIR)/log.jsonl"
```

## Checkpoints

Write `checkpoint.json` files at stage boundaries:

```json
{
  "stage": "build",
  "step": "build-resume-step",
  "status": "completed",
  "timestamp": "2024-01-15T10:30:00Z",
  "artifacts": ["resume.md", "resume.pdf"]
}
```

## Debug LLM Prompts

Keep a copy of prompt and response for debugging:

```nushell
{
    prompt_id: "resume-data-extractor.v1"
    prompt_text: $substituted_prompt
    model: "claude-3-sonnet"
    response: $model_response
    timestamp: (date now | format date "%Y-%m-%dT%H:%M:%S")
} | save --force $"($env.OUT_DIR)/prompt-debug.json"
```

## Inspect Artifact Chain

When a Task fails, inspect the full chain:

```bash
# List all artifacts
find ./tmp -name "artifact.json" -exec echo "=== {} ===" \; -exec cat {} \;

# Check for errors
find ./tmp -name "error.json" -exec echo "=== {} ===" \; -exec cat {} \;

# View logs
find ./tmp -name "log.jsonl" -exec echo "=== {} ===" \; -exec cat {} \;
```

## Environment Variables for Debugging

```bash
# Enable debug logging
export RUST_LOG=debug

# Trace SQL queries
export RUST_LOG=sqlx=trace

# Trace specific module
export RUST_LOG=dapple_prompt_flow=trace
```

## Conduit Debugging

### Debug Files

Write these to `OUT_DIR`:

**prompt-debug.json:**
```json
{
  "prompt_id": "resume-data-extractor.v1",
  "provider": "anthropic",
  "model": "claude-3-sonnet",
  "prompt_text": "SYSTEM: You are...\nUSER: Hi...",
  "response_text": "{\"name\": \"John Doe\"...}",
  "latency_ms": 1523,
  "tokens": { "input": 150, "output": 45 },
  "timestamp": "2024-01-15T10:30:00Z"
}
```

**fluent-trace.json:**
```json
{
  "conversation_id": "conv-123",
  "messages": [
    { "role": "system", "content": "..." },
    { "role": "user", "content": "..." },
    { "role": "assistant", "content": "..." }
  ]
}
```

## StepEngine Debugging

### Use `debug` Command

```nushell
let intermediate = $input | transform
$intermediate | debug  # Prints value to stdout
```

### Check `DAPPLE_METADATA`

```nushell
print $"Metadata: ($env.DAPPLE_METADATA)"
let meta = $env.DAPPLE_METADATA | from json
print $"Conduit URL: ($meta.conduit_url)"
```

### Parse Error Cache

The StepEngine caches parse errors by script hash:
- Scripts that fail to parse are cached
- Check logs for "cached parse error" messages

## Database Debugging

### Query Logging

```bash
# Enable SQLx query logging
export RUST_LOG=sqlx::query=info
```

### Check Constraints

```sql
-- List all constraints on a table
SELECT conname, pg_get_constraintdef(oid)
FROM pg_constraint
WHERE conrelid = 'web.journeys'::regclass;
```

### Inspect Data

```sql
-- Check recent records
SELECT * FROM web.journeys ORDER BY created_at DESC LIMIT 10;

-- Check foreign key references
SELECT j.object_id, p.object_id as playbook_object_id
FROM web.journeys j
JOIN web.playbooks p ON p.id = j.playbook_id;
```

## Common Debug Scenarios

### "Task returns wrong output format"

```nushell
# Add contract validation
let output = do_work
if ($output | columns | sort) != ["payload", "signals"] {
    print $"Invalid output format: ($output | columns)"
}
```

### "Can't find input artifact"

```bash
# Check environment
echo $IN_ARTIFACT
cat $IN_ARTIFACT
```

### "Conduit request fails"

```nushell
# Test conduit connectivity
try {
    "test" | summarize --max-length 10
    print "Conduit OK"
} catch {
    print $"Conduit error: ($err)"
}
```

### "SQLx compile error"

```bash
# Delete cache and regenerate (preferred: `dapple sqlx prepare`)
rm -rf .sqlx/
cargo sqlx prepare --workspace -- --all-targets
```

## Debugging Tips

1. **Start simple** - Test with minimal input first
2. **Check artifacts** - Verify each Task writes expected files
3. **Inspect logs** - Read `log.jsonl` for trace info
4. **Use checkpoints** - Track stage completion
5. **Isolate failures** - Run Tasks individually
6. **Check environment** - Verify all env vars are set
