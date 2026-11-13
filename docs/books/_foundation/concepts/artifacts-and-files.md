# Artifacts & Files

> **Audience:** Prompt Engineers · Developers · Agents

This document describes the file contracts and artifact patterns used in dapple-prompt workflows.

---

<!-- ANCHOR: artifacts -->
## Artifacts

Artifacts are files produced by Tasks: Markdown documents, PDFs, JSON data, images, or any other output. The runtime tracks artifacts and passes them between Tasks.

**Key principle:** Each Task should produce a well-defined artifact with consistent metadata.
<!-- ANCHOR_END: artifacts -->

---

<!-- ANCHOR: artifact-json -->
## artifact.json

The minimal artifact contract is `artifact.json`, written by a Task to describe its primary output:

```json
{
  "path": "./out/resume.pdf",
  "type": "application/pdf"
}
```

| Field | Description |
|-------|-------------|
| `path` | Relative path to the artifact within `OUT_DIR` |
| `type` | MIME type of the artifact |

**Best practice:** Always write `artifact.json` so downstream Tasks and the runtime can reliably locate and use produced files.
<!-- ANCHOR_END: artifact-json -->

---

<!-- ANCHOR: checkpoint -->
## checkpoint.json & Human Review

Use `checkpoint.json` to record human approvals or review decisions at Step/Stage boundaries:

```json
{
  "approved": true,
  "reviewer": "user@example.com",
  "notes": "Looks good, proceed to next stage.",
  "timestamp": "2024-01-22T10:30:00Z"
}
```

The runtime uses checkpoint files to:
- Pause Playbooks pending human review
- Resume from the last approved checkpoint
- Record audit trails of human decisions
<!-- ANCHOR_END: checkpoint -->

---

<!-- ANCHOR: logs -->
## Logging & Traces

Tasks should write structured logs and traces to `OUT_DIR`:

| File | Purpose |
|------|---------|
| `log.jsonl` | Newline-delimited JSON structured logs for machine consumption |
| `prompt-debug.json` | Records substituted prompts and raw model responses for LLM calls |
| `fluent-trace.json` | Serialized Fluent Message exchanges (system/user/assistant/tool) |

**log.jsonl format:**
```json
{"level": "info", "message": "Starting extraction", "timestamp": "2024-01-22T10:30:00Z"}
{"level": "debug", "message": "Parsed 15 entries", "count": 15, "timestamp": "2024-01-22T10:30:01Z"}
```

**prompt-debug.json format:**
```json
{
  "prompt_id": "resume-data-extractor.v1",
  "substituted_prompt": "Extract the following data...",
  "model_response": "...",
  "provider": "anthropic",
  "model": "claude-3-sonnet",
  "latency_ms": 1234
}
```
<!-- ANCHOR_END: logs -->

---

<!-- ANCHOR: async -->
## Async / pending.json

When a model call is long-running or async, Tasks should write `pending.json` with request metadata:

```json
{
  "request_id": "req-abc123",
  "status": "pending",
  "submitted_at": "2024-01-22T10:30:00Z",
  "expected_completion": "2024-01-22T10:35:00Z"
}
```

A separate worker can poll for completion and resume processing when the result is ready.
<!-- ANCHOR_END: async -->

---

<!-- ANCHOR: errors -->
## Errors & error.json

If a Task fails, write `error.json` with a structured payload for friendly diagnostics:

```json
{
  "message": "Failed to connect to PDF renderer",
  "code": 503,
  "details": {
    "service": "pdf-renderer",
    "timeout_ms": 5000
  }
}
```

| Field | Description |
|-------|-------------|
| `message` | Human-readable error description |
| `code` | Numeric error code (HTTP-style recommended) |
| `details` | Optional structured data for debugging |

The runtime uses `error.json` to display meaningful error messages to users and operators.
<!-- ANCHOR_END: errors -->

---

## File Summary

| File | Written By | Purpose |
|------|------------|---------|
| `artifact.json` | Task | Describes primary output |
| `checkpoint.json` | Runtime/Human | Records review decisions |
| `log.jsonl` | Task | Structured logs |
| `prompt-debug.json` | Task | LLM call debugging |
| `fluent-trace.json` | Task | Message exchange trace |
| `pending.json` | Task | Async request tracking |
| `error.json` | Task | Failure diagnostics |
