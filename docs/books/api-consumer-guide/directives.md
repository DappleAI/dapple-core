# Directives and Flow Control

> **Audience:** SDK Consumers · API Consumers · Integration Developers

Directives are versioned JSONLogic rules that control flow at the stage and step level. They determine whether a stage/step should execute, what to do on success or failure, and whether to retry or branch.

> For a deep dive into JSONLogic syntax, the `run` primitive, directive design patterns, and the unified flow model, see the [Journey Planner Guide - Directives and JSONLogic](../journey-planner-guide/directives.md).

---

## Directive Actions

A directive evaluates to one of these actions:

| Action | Value | Behavior |
|--------|-------|----------|
| Proceed | `"proceed"` | Continue to the next stage/step |
| Skip | `"skip"` | Skip this element entirely |
| Fail | `"fail"` | Halt the workflow with an error |
| Retry | `"retry"` | Wait `retry_delay_ms` and try again (up to `max_attempts`) |
| Loop | `"loop"` | Try again immediately, no delay (up to `max_attempts`) |
| Branch | `"{uuid}"` | Jump to the stage/step with the given `id` |

Boolean shortcuts: `true` maps to `proceed`, `false` maps to `fail`.

---

## The `run` Primitive

Directives can invoke execution via the `run` primitive:

```json
{ "run": [] }
```

This executes the attached stage/step and returns:

```json
{
  "status": "ok",
  "error_code": null,
  "data": { },
  "signals": { },
  "duration_ms": 1234
}
```

After `run`, access results via `var`:

```json
{ "var": "run.status" }
{ "var": "run.error_code" }
{ "var": "run.data.some_field" }
```

The `run` primitive is evaluated exactly once per directive execution. The engine captures the result and makes it available for subsequent checks within the same directive.

---

## Creating a Directive

`POST /api/directives/`

```json
{
  "objs": [
    {
      "name": "retry-transient-errors",
      "logic": {
        "if": [
          { "==": [{ "var": { "run": [] } }, "ok"] },
          "proceed",
          { "if": [
            { "in": [{ "var": "run.error_code" }, ["RATE_LIMITED", "TIMEOUT"]] },
            "retry",
            "fail"
          ]}
        ]
      },
      "max_attempts": 3,
      "retry_delay_ms": 2000,
      "description": "Retry up to 3 times on rate limits or timeouts"
    },
    {
      "name": "skip-if-cached",
      "logic": {
        "if": [
          { "var": "data.cache.has_result" },
          "skip",
          { "if": [
            { "==": [{ "var": { "run": [] } }, "ok"] },
            "proceed",
            "fail"
          ]}
        ]
      },
      "description": "Skip execution when a cached result already exists"
    }
  ]
}
```

**Fields:**

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Unique name (becomes the slug and path identifier) |
| `logic` | Yes | JSONLogic expression that evaluates to a directive action |
| `max_attempts` | No | Maximum retry/loop attempts (default: 1) |
| `retry_delay_ms` | No | Delay between retry attempts in milliseconds |
| `description` | No | Human-readable description |

---

## Querying Directives

`GET /api/directives/`

| Variant | Example |
|---------|---------|
| `by_object_ids` | `?query=by_object_ids&ids[0]=uuid` |
| `by_path_ids` | `?query=by_path_ids&path_ids[0]=path-uuid` |
| `all` | `?query=all&options={"limit":50}` |
| `by_slugs` | `?query=by_slugs&slugs[0]=retry-transient-errors` |

Directives also have a separate paths endpoint for version history:

```
GET /api/directive-paths/?query=by_object_ids&ids[0]=path-uuid
```

---

## Versioning Directives (PATCH)

`PATCH /api/directives/`

Update a directive by referencing an existing version's `id`. This creates a new version (patch increment) with the updated logic.

```json
{
  "objs": [
    {
      "id": "existing-directive-uuid",
      "logic": {
        "if": [
          { "==": [{ "var": { "run": [] } }, "ok"] },
          "proceed",
          { "if": [
            { "in": [{ "var": "run.error_code" }, ["RATE_LIMITED", "TIMEOUT", "SERVICE_UNAVAILABLE"]] },
            "retry",
            "fail"
          ]}
        ]
      },
      "max_attempts": 5,
      "retry_delay_ms": 3000
    }
  ]
}
```

Existing playbooks continue referencing their current directive version until explicitly updated.

---

## Attaching Directives to Playbooks

Directives attach to a playbook's stages and steps as part of the **playbook
definition**. In the guide-centric flow this attachment is authored in the guide
(via the `:::stage` / `:::step` flow-control / directive references) and applied
when the guide is ingested — it is **not** set through the `POST /api/playbooks/`
body, whose stage/step entries carry only `step_id` / `task_id` references.

A stage or step with no directive uses the default behavior: run
unconditionally, proceed on success, fail on error. The same directive can be
reused across multiple stages, steps, and playbooks.

For authoring directive attachments in a guide, see the
[Journey Planner Guide - Directives and JSONLogic](../journey-planner-guide/directives.md).

---

## Common Directive Patterns

**Retry on transient errors:**

```json
{
  "name": "retry-transient",
  "max_attempts": 3,
  "retry_delay_ms": 2000,
  "logic": {
    "if": [
      { "==": [{ "var": { "run": [] } }, "ok"] },
      "proceed",
      { "if": [
        { "in": [{ "var": "run.error_code" }, ["RATE_LIMITED", "TIMEOUT"]] },
        "retry",
        "fail"
      ]}
    ]
  }
}
```

**Skip if data already exists:**

```json
{
  "name": "skip-if-cached",
  "logic": {
    "if": [
      { "var": "data.cache.has_result" },
      "skip",
      { "if": [
        { "==": [{ "var": { "run": [] } }, "ok"] },
        "proceed",
        "fail"
      ]}
    ]
  }
}
```

**Validation gate (fail fast if required data missing):**

```json
{
  "name": "require-email",
  "logic": {
    "if": [
      { "missing": ["data.user.email"] },
      "fail",
      { "if": [
        { "==": [{ "var": { "run": [] } }, "ok"] },
        "proceed",
        "fail"
      ]}
    ]
  }
}
```

---

## Branch Scope Rules

- **Stage directives** can branch to any stage in the playbook
- **Step directives** can only branch to steps within the same stage

Invalid branch targets cause the workflow to fail with a clear error message.

---

## Next Steps

- [API Patterns and Semantics](./api-patterns.md) -- batch operations, response formats, pagination
- [Endpoint Reference](./endpoint-reference.md) -- full endpoint table
- [Error Handling](./error-handling.md) -- error codes and resolution
- [Journey Planner Guide - Directives](../journey-planner-guide/directives.md) -- deep dive into JSONLogic syntax and the flow model
