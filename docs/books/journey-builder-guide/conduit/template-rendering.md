# Template Rendering

> **Audience:** Backend Developers · Prompt Engineers

The `TemplateService` uses Handlebars to render prompt templates with context data.

## Template Syntax

```handlebars
{{! System message }}
You are a helpful assistant that extracts data from resumes.

{{! Using context variables }}
User name: {{user_name}}
Task: {{task}}

{{! Conditional rendering }}
{{#if company_name}}
Company: {{company_name}}
{{/if}}

{{! Visible slots }}
{{#each _visible_slots}}
Available slot: {{this}}
{{/each}}
```

## Template Context

The template context is built from:

1. **Context data** — Data from the attempt (e.g., debrief responses)
2. **Visible slots** — Value slots configured for the task

```json
{
  "user_name": "Alice",
  "task": "summarize",
  "_visible_slots": ["contact_info", "work_history"],
  "contact_info": {
    "name": "contact_info",
    "value_kind": "text",
    "object_id": "uuid..."
  }
}
```

## Missing Variables

By default, missing variables render as empty strings (not errors). This allows templates to work with optional data.

```handlebars
Hello, {{name}}!  {{! If name is missing, renders as "Hello, !" }}
```

## SSE Streaming

The conduit returns responses as Server-Sent Events (SSE) for real-time streaming.

### Event Types

| Event | Description | Payload |
|-------|-------------|---------|
| `started` | Request started | `{ request_id, engine, prompt_object_id }` |
| `token` | Output token | `{ token }` |
| `done` | Completion | `{ output_tokens, input_tokens, latency_ms, finish_reason }` |
| `error` | Error occurred | `{ code, message, retryable }` |

### Example SSE Stream

```
data: {"event":"started","request_id":"abc123","engine":"claude-3-sonnet"}

data: {"event":"token","token":"Hello"}

data: {"event":"token","token":" world"}

data: {"event":"token","token":"!"}

data: {"event":"done","output_tokens":3,"input_tokens":150,"latency_ms":1523,"finish_reason":"stop"}
```

### Consuming SSE in Nushell

```nushell
# The prompt command handles SSE internally
{ input: $data } | prompt send "my-prompt"
```

## Using Prompts in Tasks

### Environment Variables

When the runtime executes your Task, it provides:

| Variable | Description | Example |
|----------|-------------|---------|
| `PROMPT_ID` | The prompt template to use | `resume-data-extractor.v1` |
| `CONDUIT_OPTIONS` | JSON blob with provider/model config | `{"model":"claude-3-sonnet"}` |
| `CONDUIT_URL` | Conduit service URL | `http://localhost:8081` |

### Example Task

```nushell
#!/usr/bin/env nu

# TASK: extract-resume-data
# PROMPT_ID: resume-data-extractor.v1

def main [] {
    let input = $in

    # Send prompt via conduit
    let response = ($input | prompt send "resume-data-extractor.v1")

    # Return contract-compliant output
    {
        payload: $response,
        signals: {}
    }
}
```

## Best Practices

### For Template Design

1. **Use optional sections** — Don't require all variables
2. **Include examples** — Help the model understand expected format
3. **Keep context focused** — Don't include unnecessary data
4. **Test with different engines** — Behavior may vary by provider

### For Prompt Authors

1. **Design prompts to be provider-agnostic** — The conduit may route to different providers
2. **Always include overlay_id in logs** — For debugging and billing correlation
3. **Handle async gracefully** — Write `pending.json` for long-running calls
4. **Check response format** — Providers may return differently formatted errors
