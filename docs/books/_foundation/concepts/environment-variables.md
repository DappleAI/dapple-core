# Environment Variables

> **Audience:** Prompt Engineers · Developers · Agents

This document describes the runtime environment variables available to Tasks during execution.

---

<!-- ANCHOR: runtime-env-vars -->
## Runtime Environment Variables

The runtime exposes these environment variables to Tasks:

| Variable | Description |
|----------|-------------|
| `OUT_DIR` | Directory where Tasks should write artifacts, logs, and traces |
| `IN_ARTIFACT` | Path to the previous Task's artifact (if any) |
| `PLAYBOOK_ID` | Identifier for the current Playbook |
| `STAGE_ID` | Identifier for the current Stage |
| `STEP_ID` | Identifier for the current Step |
| `TASK_ID` | Identifier for the current Task |
| `PROMPT_ID` | Prompt identifier when a Task invokes prompts via the conduit |
| `CONDUIT_OPTIONS` | JSON blob with provider/model/options for conduit calls |
<!-- ANCHOR_END: runtime-env-vars -->

---

<!-- ANCHOR: out-dir -->
## OUT_DIR

The `OUT_DIR` variable points to the directory where the current Task should write all its outputs:

```bash
# Example Task script
echo '{"path": "./resume.pdf", "type": "application/pdf"}' > "$OUT_DIR/artifact.json"
cp /tmp/rendered.pdf "$OUT_DIR/resume.pdf"
```

**Contents typically written to OUT_DIR:**
- `artifact.json`: Primary artifact metadata
- The actual artifact files (PDFs, JSON, etc.)
- `log.jsonl`: Structured logs
- `prompt-debug.json`: LLM debugging info
- `error.json`: Error details on failure
<!-- ANCHOR_END: out-dir -->

---

<!-- ANCHOR: in-artifact -->
## IN_ARTIFACT

The `IN_ARTIFACT` variable points to the previous Task's primary artifact, enabling data flow between Tasks:

```bash
# Read the previous task's artifact
PREVIOUS_DATA=$(cat "$IN_ARTIFACT")

# Process it
echo "$PREVIOUS_DATA" | jq '.name' > "$OUT_DIR/name.txt"
```

**Note:** `IN_ARTIFACT` may be unset if this is the first Task in a Step or if the previous Task didn't produce an artifact.
<!-- ANCHOR_END: in-artifact -->

---

<!-- ANCHOR: identifiers -->
## Identifiers

Stable identifiers are exposed for observability, logging, and correlation:

| Variable | Example Value | Use Case |
|----------|---------------|----------|
| `PLAYBOOK_ID` | `build-resume-playbook` | Identify which playbook is running |
| `STAGE_ID` | `data-collection` | Identify current stage |
| `STEP_ID` | `build-resume-step` | Identify current step |
| `TASK_ID` | `format-resume` | Identify current task |

Include these in logs and traces for debugging:

```json
{
  "level": "info",
  "message": "Starting extraction",
  "playbook_id": "build-resume-playbook",
  "step_id": "build-resume-step",
  "task_id": "format-resume"
}
```
<!-- ANCHOR_END: identifiers -->

---

<!-- ANCHOR: conduit-vars -->
## Conduit Variables

When a Task invokes prompts via the conduit:

| Variable | Description |
|----------|-------------|
| `PROMPT_ID` | The prompt template identifier (e.g., `resume-data-extractor.v1`) |
| `CONDUIT_OPTIONS` | JSON blob with provider, model, and options |

**CONDUIT_OPTIONS example:**

```json
{
  "provider": "anthropic",
  "model": "claude-3-sonnet",
  "max_tokens": 4096,
  "temperature": 0.7
}
```

Tasks can read `CONDUIT_OPTIONS` to understand the configured LLM settings, or pass it through to conduit calls.
<!-- ANCHOR_END: conduit-vars -->
