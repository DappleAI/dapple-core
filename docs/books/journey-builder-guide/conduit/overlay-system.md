# Overlay System

> **Audience:** Backend Developers · Platform Engineers

Overlays let you change routing behavior without editing prompts.

## Scope Precedence

| Scope | Precedence | Use Case |
|-------|------------|----------|
| Prompt | Highest | Override for specific prompt |
| Playbook | High | Override for all prompts in a playbook |
| Pack | Medium | Override for all prompts in a pack |
| Global | Lowest | Platform-wide defaults |

## Visible Slots

The `OverlayService` resolves which value slots are visible to a prompt:

```rust
let visible_slots = overlay_service
    .resolve_visible_slots(playbook_task_id)
    .await?;
```

Visible slots are injected into the template context as `_visible_slots`:

```handlebars
{{#each _visible_slots}}
Available slot: {{this}}
{{/each}}
```

## Binding Blinders (task-context filter)

The overlay cascade also filters the **conduit-facing binding context** — the
`## Bindings` section (and derived asset list + brief values) of the
`# Dispatch Context` system message. The filter runs **server-side in
dapple-prompt-api's `resolve_task_context`**, so the payload the conduit
fetches is already blinded; the conduit itself needs no overlay knowledge for
this path.

The presence rule (via `select_overlay_cascade_for_task`, which keeps masked
rows so the two cases are distinguishable):

- **Zero overlays anywhere on the task's stage→step→task cascade** → the full
  journey binding pool stays visible. Blinders are opt-in per node; guides
  without overlays behave exactly as before.
- **Any overlays on the cascade** → only the overlay-exposed slots survive
  into the conduit's view (narrowest-wins resolution, same cascade as gating).

Two deliberate boundaries:

- **The engine is never blinded.** `$in.bindings.*` seeding in the dispatch
  runner stays journey-chain-wide and order-independent (the flow crate's
  "Binding read availability" invariant). Blinders act only at the LLM-facing
  render boundary.
- **The task script's own `prompt send` pipeline is not covered.** Whatever a
  script pipes in arrives as `context_data`; that channel is author-owned.
  `## Prior Task Outcomes` summaries are also unfiltered in V1.

The `_visible_slots` template stub channel above is additive metadata (names
and kinds, never values) and treats "no overlays" as an empty list — that
asymmetry is intentional; the stub is not a withholding mechanism.

## Use Cases

- Use cheaper model for development/testing
- Route to different provider by region
- Enable debug logging for specific prompts
- Keep distractor bindings out of a prompt's view (blinders — see above)

## Tracing & Storage

### Prompt Response Storage

Each prompt execution is stored in the database:

| Field | Description |
|-------|-------------|
| `attempt_id` | Links to Journey attempt |
| `prompt_id` | Which prompt was executed |
| `playbook_task_id` | Task context |
| `lineage_key`, `provider_key` | Backend used |
| `request_messages` | Rendered prompt messages (JSON) |
| `response_content` | Complete response text |
| `input_tokens`, `output_tokens` | Token counts |
| `latency_ms` | Execution time |
| `finish_reason` | Why generation stopped |
| `started_at`, `completed_at` | Timestamps |

### Debug Files

Write these files to `OUT_DIR` for debugging:

**prompt-debug.json:**
```json
{
  "prompt_id": "resume-data-extractor.v1",
  "provider": "anthropic",
  "model": "claude-3-sonnet",
  "prompt_text": "SYSTEM: You are...\nUSER: Hi, I'm John...",
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
    { "role": "system", "content": "You are a helpful assistant..." },
    { "role": "user", "content": "Hi, I'm John Doe..." },
    { "role": "assistant", "content": "{\"name\": \"John Doe\"...}" }
  ],
  "timestamp": "2024-01-15T10:30:00Z"
}
```

## Error Handling

### Error Types

| Code | Description | Retryable |
|------|-------------|-----------|
| `model_error` | Failed to resolve model config | No |
| `context_error` | Failed to fetch attempt context | No |
| `overlay_error` | Failed to resolve overlays | No |
| `template_error` | Failed to render prompt | No |
| `llm_error` | Backend execution failed | Yes |
| `stream_error` | Streaming failed mid-response | Yes |

### Handling Errors in Tasks

```nushell
def main [] {
    try {
        let response = ($input | prompt send "my-prompt")
        { payload: $response, signals: {} }
    } catch { |err|
        # Check if retryable
        if ($err | str contains "retryable") {
            # Try again with retry command
            let response = ($input | prompt retry)
            { payload: $response, signals: {} }
        } else {
            # Write error and fail
            { message: $err.msg, code: 500 } | to json | save $"($env.OUT_DIR)/error.json"
            { payload: null, signals: { error: true } }
        }
    }
}
```

## Best Practices for Production

1. **Monitor latency** — Track `latency_ms` in responses
2. **Check token usage** — Compare `input_tokens` vs `output_tokens`
3. **Review stored responses** — Use database for analysis
4. **Set up fallbacks** — Configure multiple backends
