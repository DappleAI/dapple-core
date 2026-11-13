# Directives and JSONLogic

> **Audience:** :wrench: Prompt Engineers · :clipboard: Product · :robot: Agents

Before you write a single line of Nushell, it's worth understanding the simpler tool in the box: JSONLogic. Many workflows don't need scripting at all — they just need to check a value, transform some data, or decide whether to proceed. That's what JSONLogic is for.

This chapter covers:
1. JSONLogic basics — the declarative way to express logic
2. Directives — how JSONLogic controls flow at the stage and step level
3. When to reach for Nushell — and when not to

> **Terminology note:** This chapter covers **flow-control directives** — JSONLogic rules that control stage/step execution (proceed, skip, fail, retry, loop, branch). The Guide system also uses the term "directive type" to classify **block content** (prose, stage, step, nushell, ref). These are unrelated systems. For block directive types used in guide authoring, see [Directive Blocks](./directive-blocks.md).

---

## What is JSONLogic?

[JSONLogic](https://jsonlogic.com/) is a way to write rules as JSON. Instead of code, you have data that *describes* logic. The runtime evaluates it against your workflow state and returns a result.

**Why use it?**
- **Declarative** — You describe *what* you want, not *how* to compute it
- **Portable** — It's just JSON; store it in a database, send it over the wire
- **Fast** — No script parsing, no VM startup, just data traversal
- **Safe** — No arbitrary code execution, only predefined operators

**Example:**

```json
{ "==": [{ "var": "user.role" }, "admin"] }
```

This checks if `user.role` equals `"admin"`. That's it. No imports, no functions, no semicolons.

---

## JSONLogic Operators

Here are the operators you'll use most often:

### Data Access

| Operator | Description | Example |
|----------|-------------|---------|
| `var` | Get a value from the data | `{ "var": "user.email" }` |
| `missing` | Check if keys are missing | `{ "missing": ["user.email", "user.name"] }` |
| `missing_some` | Check if at least N keys are missing | `{ "missing_some": [1, ["email", "phone"]] }` |

### Comparison

| Operator | Description | Example |
|----------|-------------|---------|
| `==` | Loose equality | `{ "==": [1, "1"] }` → true |
| `===` | Strict equality | `{ "===": [1, "1"] }` → false |
| `!=`, `!==` | Inequality | `{ "!=": [1, 2] }` → true |
| `<`, `<=`, `>`, `>=` | Numeric comparison | `{ ">": [5, 3] }` → true |

### Logic

| Operator | Description | Example |
|----------|-------------|---------|
| `and` | All must be truthy | `{ "and": [true, true] }` → true |
| `or` | At least one truthy | `{ "or": [false, true] }` → true |
| `!` | Negation | `{ "!": false }` → true |
| `!!` | Double negation (truthy check) | `{ "!!": "hello" }` → true |
| `if` | Conditional | `{ "if": [condition, then, else] }` |

### String Operations

| Operator | Description | Example |
|----------|-------------|---------|
| `cat` | Concatenate strings | `{ "cat": ["Hello, ", { "var": "name" }] }` |
| `substr` | Substring | `{ "substr": ["hello", 0, 2] }` → "he" |
| `in` | Substring check | `{ "in": ["@", "user@example.com"] }` → true |

### Array Operations

| Operator | Description | Example |
|----------|-------------|---------|
| `in` | Value in array | `{ "in": ["admin", { "var": "roles" }] }` |
| `merge` | Combine arrays | `{ "merge": [[1, 2], [3, 4]] }` → [1, 2, 3, 4] |
| `map` | Transform each element | `{ "map": [[1, 2, 3], { "*": [{ "var": "" }, 2] }] }` |
| `filter` | Keep matching elements | `{ "filter": [[1, 2, 3, 4], { ">": [{ "var": "" }, 2] }] }` |
| `reduce` | Accumulate to single value | `{ "reduce": [[1, 2, 3], { "+": [{ "var": "current" }, { "var": "accumulator" }] }, 0] }` |

### Arithmetic

| Operator | Description | Example |
|----------|-------------|---------|
| `+`, `-`, `*`, `/` | Basic math | `{ "+": [1, 2, 3] }` → 6 |
| `%` | Modulo | `{ "%": [5, 2] }` → 1 |
| `min`, `max` | Min/max of values | `{ "max": [1, 5, 3] }` → 5 |

---

## JSONLogic in Workflows

In dapple-prompt, JSONLogic appears in two places:

1. **Tasks** — `map` and `validate` handlers use JSONLogic for data transformation and validation
2. **Directives** — Control whether/how stages and steps execute

### Map Tasks

Transform data from one shape to another:

```json
{
  "id": "normalize-user",
  "function": "map",
  "input": {
    "mappings": [
      {
        "target": "data.normalized.full_name",
        "logic": { "cat": [{ "var": "data.user.first" }, " ", { "var": "data.user.last" }] }
      },
      {
        "target": "data.normalized.is_premium",
        "logic": { "in": ["premium", { "var": "data.user.roles" }] }
      }
    ]
  }
}
```

### Validate Tasks

Check that data meets requirements:

```json
{
  "id": "validate-input",
  "function": "validate",
  "input": {
    "rules": [
      {
        "path": "data.user.email",
        "logic": { "in": ["@", { "var": "data.user.email" }] },
        "message": "Email must contain @"
      },
      {
        "path": "data.user.age",
        "logic": { ">=": [{ "var": "data.user.age" }, 18] },
        "message": "User must be 18 or older"
      }
    ]
  }
}
```

---

## Directives: Flow Control with JSONLogic

While tasks use JSONLogic for data work, **directives** use it for flow control. A directive wraps around a stage or step and decides:

- Should we execute at all? (skip)
- Did it work? (proceed or fail)
- Should we try again? (retry or loop)
- Should we go somewhere else? (branch)

### The `run` Primitive

Directives introduce a special operator: `run`. This actually executes the stage or step:

```json
{ "run": [] }
```

It returns:
```json
{
  "status": "ok",        // or "error", "timeout", "canceled"
  "error_code": null,    // or "RATE_LIMITED", "TIMEOUT", etc.
  "data": { ... },       // Output from the execution
  "signals": { ... },    // Control signals
  "duration_ms": 1234
}
```

After `run` executes, you access results via `var`:
```json
{ "var": "run.status" }
{ "var": "run.data.classification" }
{ "var": "run.signals.needs_review" }
```

> **:robot: Implementation Agent Note:** The `run` primitive is evaluated exactly once per directive execution. The engine captures its result and makes it available via `var` for subsequent checks. You cannot call `run` multiple times in the same directive.

### Directive Actions

A directive must return one of these actions:

| Action | Meaning |
|--------|---------|
| `"proceed"` | Continue to the next stage/step |
| `"skip"` | Skip this element entirely |
| `"fail"` | Halt the workflow with an error |
| `"retry"` | Wait and try again (respects `retry_delay_ms`) |
| `"loop"` | Try again immediately (no delay) |
| `"{uuid}"` | Branch to a different stage/step |

> **:robot: API Consumer Agent Note:** When your directive returns a branch UUID, the engine validates it exists and is reachable from the current scope. Invalid UUIDs cause the workflow to fail with a clear error message.

### Example: Skip if Cached

```json
{
  "if": [
    { "var": "data.cache.has_result" },
    "skip",
    { "if": [
      { "==": [{ "var": { "run": [] }.status }, "ok"] },
      "proceed",
      "fail"
    ]}
  ]
}
```

Translation: If we have a cached result, skip. Otherwise run, and proceed if successful.

### Example: Retry on Rate Limit

With `max_attempts: 3` and `retry_delay_ms: 2000`:

```json
{
  "if": [
    { "==": [{ "var": { "run": [] }.status }, "ok"] },
    "proceed",
    { "if": [
      { "in": [{ "var": "run.error_code" }, ["RATE_LIMITED", "TIMEOUT"]] },
      "retry",
      "fail"
    ]}
  ]
}
```

Translation: Run. If OK, proceed. If rate-limited or timed out, retry (up to 3 times with 2s delay). Otherwise fail.

### Example: Branch by Result

```json
{
  "if": [
    { "==": [{ "var": { "run": [] }.status }, "error"] },
    "fail",
    { "if": [
      { "==": [{ "var": "run.data.tier" }, "premium"] },
      "550e8400-e29b-41d4-a716-premium-stage",
      "proceed"
    ]}
  ]
}
```

Translation: Run. If error, fail. If the result says "premium", branch to the premium stage. Otherwise proceed normally.

### Default Behavior

When no directive is attached, the engine behaves as if you wrote:

```json
{
  "if": [
    { "==": [{ "var": { "run": [] }.status }, "ok"] },
    "proceed",
    "fail"
  ]
}
```

Run unconditionally. Proceed on success. Fail on error. Simple.

---

## When to Use Nushell

JSONLogic is great for:
- Checking values
- Simple transformations
- Boolean decisions
- Validation rules

But it gets awkward when you need:
- Complex data manipulation (grouping, sorting, aggregation)
- String formatting beyond concatenation
- External data (reading files, making HTTP calls)
- Iterative processing with side effects
- Anything that feels like "programming"

### The Weight of Nushell

Nushell is a full shell with:
- Parser startup
- VM initialization
- Security policy evaluation
- Script execution

For checking if `data.user.email` contains "@", that's overkill. JSONLogic does it in microseconds with zero overhead.

**Rule of thumb:** If your logic fits in a JSONLogic expression without contortion, use JSONLogic. Reach for Nushell when you're fighting the declarative model.

### Examples

**Use JSONLogic:**
```json
// Check a value
{ "==": [{ "var": "data.status" }, "active"] }

// Combine strings
{ "cat": [{ "var": "first" }, " ", { "var": "last" }] }

// Validate presence
{ "!": { "missing": ["email", "name"] } }
```

**Use Nushell:**
```nushell
# Group and aggregate
$in | group-by category | each { |g| { category: $g.0, count: ($g.1 | length) } }

# Complex string formatting
$in | each { |row| $"($row.name) <($row.email)> - ($row.role | str upcase)" }

# Data enrichment with computation
$in | insert score { |row| ($row.views * 0.1) + ($row.likes * 0.5) }
```

---

## Unified Flow Model

Here's the key insight: **the flow model is the same regardless of task type**.

Whether your task is:
- A JSONLogic `map` task
- A JSONLogic `validate` task
- A Nushell pipeline task
- A prompt/LLM task

...the directive wrapping it works identically. The `run` primitive executes the task, returns a result, and the directive decides what happens next.

```
┌─────────────────────────────────────────────┐
│               Directive                      │
│  ┌────────────────────────────────────────┐ │
│  │ Pre-check: Should we run?              │ │
│  │   { "if": [{ "var": "data.skip" },     │ │
│  │           "skip",                       │ │
│  └──────────────┬─────────────────────────┘ │
│                 ▼                            │
│  ┌────────────────────────────────────────┐ │
│  │ { "run": [] }                          │ │
│  │   ↓                                     │ │
│  │ ┌────────────────────────────────────┐ │ │
│  │ │ Task (JSONLogic, Nushell, Prompt)  │ │ │
│  │ └────────────────────────────────────┘ │ │
│  │   ↓                                     │ │
│  │ Returns: { status, data, signals }     │ │
│  └──────────────┬─────────────────────────┘ │
│                 ▼                            │
│  ┌────────────────────────────────────────┐ │
│  │ Post-check: What next?                 │ │
│  │   { "if": [success?, "proceed",        │ │
│  │           retry?, "retry", "fail"] }   │ │
│  └────────────────────────────────────────┘ │
└─────────────────────────────────────────────┘
```

This uniformity means:
- You can swap task implementations without changing flow logic
- Directives are reusable across different task types
- Testing flow behavior is independent of task complexity

---

## Creating and Attaching Directives

Directives are versioned objects. Create them independently, then attach to stages or steps when building playbooks.

In guide-authored workflows, directives are referenced by name in `:::stage` and `:::step` attributes. In programmatic workflows, directives are created via the API and attached by `object_id`.

Directives are reusable across multiple stages, steps, and playbooks. When updating a directive, a new version is created (PATCH increments patch version). Existing playbooks continue using their referenced version until explicitly updated.

> For API endpoints and SDK methods for creating, querying, versioning, and attaching directives, see the [API Consumer Guide - Directives and Flow Control](../api-consumer-guide/directives.md).

---

## Branch Scope Rules

When branching to a different stage or step:

- **Stage directives** can branch to any stage in the playbook
- **Step directives** can only branch to steps within the same stage

This keeps the execution model predictable — you can't jump across stage boundaries from a step.

> **:robot: Playbook Authoring Agent Note:** When building playbooks programmatically, always use the stage's `object_id` (not internal `id`) for branch targets. The engine resolves these UUIDs at runtime. Step-level branches must reference steps within the same stage — the engine validates this constraint when the playbook is created.

---

## Best Practices

### Keep Directives Simple

A directive should express one flow pattern. If your JSONLogic is getting deeply nested, consider:
- Breaking into multiple stages with simpler directives
- Moving complex logic into a task

### Name Directives Descriptively

```
✓ retry-on-rate-limit
✓ skip-if-cached
✓ validate-then-branch

✗ flow-1
✗ my-directive
✗ handler
```

### Test Directives Independently

Since directives are versioned objects, you can:
- Create test directives with mock `run` results
- Verify action outputs without running actual tasks
- Version directives separately from playbooks

### Use `max_attempts` Wisely

- **retry** respects `max_attempts` and `retry_delay_ms`
- **loop** respects `max_attempts` but has no delay
- When attempts are exhausted, the directive returns `"fail"`

Set reasonable limits. Infinite retries are a recipe for stuck workflows.

---

## Next Steps

Now that you understand the declarative foundation:

1. **[Writing Nushell Tasks](./writing-nushell-tasks.md)** -- When you need full scripting power
2. **[Custom Nushell Commands](./custom-nu-commands.md)** -- Available commands in the runtime
3. **[Resume Guide Walkthrough](./resume-guide-walkthrough.md)** -- See it all in action
