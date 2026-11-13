# Prompt Execution and Routing

> **Audience:** :wrench: Prompt Engineers
>
> *Product readers can skip this chapter.*

This chapter explains, at an author-facing level, how the prompt conduit routes your prompts to LLM providers and streams back results.

---

## What the Conduit Does

The conduit is a **provider-agnostic routing layer** that sits between your Playbook tasks and LLM providers. When a task calls `prompt send`, the conduit:

1. **Resolves** the inference profile to determine which model and provider to use
2. **Renders** the prompt template with context data (Handlebars)
3. **Routes** the rendered prompt to the appropriate LLM backend
4. **Streams** the response back via Server-Sent Events (SSE)
5. **Stores** all interactions for debugging and replay

---

## Inference Profile Resolution

Model configuration for guide-authored workflows is defined via `:::inference_profile` directives. See [Defining Inference Profiles](./defining-inference-profiles.md) for the authoring side.

At runtime, the conduit resolves inference profiles using the following logic:

1. Look up the profile configs attached to the journey's inference profile
2. Match against the prompt's path object ID
3. Fall back through progressively less specific matches until a compatible backend is found

### Release Chain

Template resolution uses a fallback chain from most specific to least specific:

```
@provider:lineage/tier:release    (most specific)
@provider:lineage/tier
lineage/tier:release
lineage/tier
lineage                           (least specific)
```

---

## LLM Backends

The conduit supports multiple LLM backends, selected based on the `lineage_key` from the resolved inference profile.

| Backend | Lineage Key | Requirements |
|---------|-------------|--------------|
| Anthropic | `claude-3-5`, `claude-4-6` | `ANTHROPIC_API_KEY` environment variable |
| Mistral | `mistral` | `MISTRAL_ENABLED=true` |
| Stub | `stub` | None (always available, echoes input for testing) |

If no backend matches the resolved configuration, the conduit falls back to the stub backend.

---

## Prompt Template Rendering

The conduit renders prompt templates using Handlebars syntax.

### Template Syntax

```handlebars
{{! System message }}
You are a helpful assistant that extracts data from resumes.

{{! Context variables }}
User name: {{user_name}}
Task: {{task}}

{{! Conditional sections }}
{{#if company_name}}
Company: {{company_name}}
{{/if}}

{{! Visible slots }}
{{#each _visible_slots}}
Available slot: {{this}}
{{/each}}
```

### Template Context

The template context is built from:

1. **Context data** -- Data from the attempt (e.g., debrief responses)
2. **Visible slots** -- Value slots configured for the task

Missing variables render as empty strings (not errors), allowing templates to work with optional data.

---

## SSE Streaming

The conduit returns responses as Server-Sent Events for real-time streaming.

### Event Types

| Event | Description | Payload |
|-------|-------------|---------|
| `started` | Request started | `{ request_id, engine, prompt_object_id }` |
| `token` | Output token | `{ token }` |
| `done` | Completion | `{ output_tokens, input_tokens, latency_ms, finish_reason }` |
| `error` | Error occurred | `{ code, message, retryable }` |

### Consuming SSE in Nushell

The `prompt send` command handles SSE internally:

```nushell
{ input: $data } | prompt send "my-prompt"
```

---

## Using Prompt Commands in Tasks

The conduit is accessed from tasks via custom Nushell commands. See [Custom Nushell Commands](./custom-nu-commands.md) for full documentation.

| Command | Description |
|---------|-------------|
| `prompt send` | Send a prompt to the conduit and receive a response |
| `prompt retry` | Retry a failed prompt with the same context |
| `prompt reset` | Reset the conversation state |

### Example Task

```nushell
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

### Environment Variables

When the runtime executes your task, it provides:

| Variable | Description |
|----------|-------------|
| `PROMPT_ID` | The prompt template to use |
| `CONDUIT_OPTIONS` | JSON blob with provider/model config |
| `CONDUIT_URL` | Conduit service URL |

---

## Error Handling

### Error Types

| Code | Description | Retryable |
|------|-------------|-----------|
| `engine_error` | Failed to resolve engine config | No |
| `context_error` | Failed to fetch attempt context | No |
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
        if ($err | str contains "retryable") {
            let response = ($input | prompt retry)
            { payload: $response, signals: {} }
        } else {
            { message: $err.msg, code: 500 } | to json | save $"($env.OUT_DIR)/error.json"
            { payload: null, signals: { error: true } }
        }
    }
}
```

---

## Configuration

### LLM Backend Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ANTHROPIC_API_KEY` | (none) | Enable Anthropic backend |
| `MISTRAL_ENABLED` | `false` | Enable local Mistral inference |
| `MISTRAL_MODEL_ID` | `mistralai/Mistral-7B-Instruct-v0.3` | HuggingFace model ID |
| `MISTRAL_DEVICE` | auto-detected | `cpu`, `cuda:0`, or `metal` |
| `MISTRAL_DTYPE` | `f16` | `f16`, `bf16`, or `f32` |
| `MISTRAL_MAX_CONTEXT` | `8192` | Maximum context length |
| `HF_TOKEN` | (none) | HuggingFace token for gated models |

### Starting the Conduit

```bash
# With Anthropic
ANTHROPIC_API_KEY=sk-ant-... cargo run -p dapple-prompt-conduit

# With local Mistral
MISTRAL_ENABLED=true cargo run -p dapple-prompt-conduit

# Stub only (for testing)
cargo run -p dapple-prompt-conduit
```

---

## Best Practices

### For Prompt Authors

1. **Design prompts to be provider-agnostic** -- The conduit may route to different providers
2. **Use optional template sections** -- Don't require all variables
3. **Include examples in prompts** -- Help the model understand expected format
4. **Keep context focused** -- Don't include unnecessary data
5. **Test with different backends** -- Behavior may vary by provider

### For Production

1. **Monitor latency** -- Track `latency_ms` in `done` events
2. **Check token usage** -- Compare `input_tokens` vs `output_tokens`
3. **Set up fallbacks** -- Configure multiple backends
4. **Review stored responses** -- Use the database for analysis and debugging

---

## Debug Files

Write these files to `OUT_DIR` for debugging:

**prompt-debug.json:**
```json
{
  "prompt_id": "resume-data-extractor.v1",
  "provider": "anthropic",
  "model": "claude-sonnet-4-5-20250929",
  "prompt_text": "SYSTEM: You are...\nUSER: Hi, I'm John...",
  "response_text": "{\"name\": \"John Doe\"...}",
  "latency_ms": 1523,
  "tokens": { "input": 150, "output": 45 },
  "timestamp": "2024-01-15T10:30:00Z"
}
```

---

## Notes

- OpenAI backend is planned but not yet implemented.
- Additional LLM backends (Cohere, HuggingFace) are on the roadmap.
