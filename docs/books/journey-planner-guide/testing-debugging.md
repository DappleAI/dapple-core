# Testing & Debugging Playbooks

> **Audience:** :wrench: Prompt Engineers · :robot: Agents

Author-level testing guidance for Playbooks and Tasks.

---

## Local Testing

### Test Individual Tasks

Run each Task script locally with sample JSON input:

```bash
# Set up test environment
export OUT_DIR=./tmp/out
mkdir -p $OUT_DIR

# Run with sample input
echo '{"name": "Test User", "email": "test@example.com"}' | nu collect-personal-data.nu

# Verify outputs
cat $OUT_DIR/artifact.json
cat $OUT_DIR/log.jsonl
```

**Verify each script writes:**
- `artifact.json` with `path` and `type` fields
- `log.jsonl` with structured log entries
- `error.json` on failure (with `message` and `code`)

### Test Task Chains

Simulate Step execution by running Tasks in order, passing `IN_ARTIFACT` from the previous `artifact.json`:

```bash
# Task 1
echo '{"name": "Test"}' | OUT_DIR=./tmp/t1 nu task1.nu

# Task 2 (reads from Task 1)
IN_ARTIFACT=./tmp/t1/artifact.json OUT_DIR=./tmp/t2 nu task2.nu

# Task 3 (reads from Task 2)
IN_ARTIFACT=./tmp/t2/artifact.json OUT_DIR=./tmp/t3 nu task3.nu
```

---

## Integration Smoke Test

Before registering a Playbook, run an end-to-end smoke test:

```nushell
#!/usr/bin/env nu

# smoke-test.nu - Run full pipeline locally

def main [input_file: string] {
    let base = "./tmp/smoke"
    rm -rf $base
    mkdir $base

    # Task 1
    print "Running collect-personal-data..."
    open $input_file | nu collect-personal-data.nu
    let t1_out = $"($base)/t1"

    # Task 2
    print "Running format-resume..."
    IN_ARTIFACT=$"($t1_out)/artifact.json" OUT_DIR=$"($base)/t2" nu format-resume.nu

    # Task 3
    print "Running render-pdf..."
    IN_ARTIFACT=$"($base)/t2/artifact.json" OUT_DIR=$"($base)/t3" nu render-pdf.nu

    # Verify
    let final = open $"($base)/t3/artifact.json"
    if ($final.path | path exists) {
        print $"Success! Final artifact: ($final.path)"
    } else {
        print "FAILED: Final artifact not found"
        exit 1
    }
}
```

---

## Debugging Tips

### Preserve Logs

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

### Use Checkpoints

Write `checkpoint.json` files at stage boundaries for human review and resuming:

```json
{
  "stage": "build",
  "step": "build-resume-step",
  "status": "completed",
  "timestamp": "2024-01-15T10:30:00Z",
  "artifacts": ["resume.md", "resume.pdf"]
}
```

### Debug LLM Prompts

When debugging prompts, keep a copy of the full prompt and model response:

```nushell
# Write prompt debug info
{
    prompt_id: "resume-data-extractor.v1"
    prompt_text: $substituted_prompt
    model: "gpt-4"
    response: $model_response
    timestamp: (date now | format date "%Y-%m-%dT%H:%M:%S")
} | save --force $"($env.OUT_DIR)/prompt-debug.json"
```

### Inspect Artifact Chain

When a Task fails, inspect the full artifact chain:

```bash
# List all artifacts
find ./tmp -name "artifact.json" -exec echo "=== {} ===" \; -exec cat {} \;

# Check for errors
find ./tmp -name "error.json" -exec echo "=== {} ===" \; -exec cat {} \;

# View logs
find ./tmp -name "log.jsonl" -exec echo "=== {} ===" \; -exec cat {} \;
```

---

## Pre-Release Checklist

Before publishing a Playbook or BriefPack:

- [ ] **Sample inputs included** — Provide example JSON for each Task
- [ ] **Required binaries documented** — List dependencies (e.g., `pandoc`, `jq`)
- [ ] **Privacy/PII review completed** — Mask sensitive data in examples and logs
- [ ] **Artifact schema validated** — All Tasks write valid `artifact.json`
- [ ] **Error paths tested** — Verify `error.json` is written on failure
- [ ] **Idempotency verified** — Tasks can be safely re-run
- [ ] **End-to-end smoke test passes** — Full pipeline works with sample data

---

## Common Issues

| Symptom | Likely Cause | Fix |
|---------|--------------|-----|
| Task exits 0 but no artifact | Forgot to write `artifact.json` | Add artifact write at end of Task |
| Next Task can't find input | Wrong `IN_ARTIFACT` path | Check previous Task's `artifact.json` path |
| JSON parse error | Malformed input | Validate JSON structure before processing |
| `error.json` not created | Task crashed before error handling | Add try/catch or early validation |
| Logs missing | Wrote to wrong directory | Always use `$env.OUT_DIR` |
| Task succeeds but the finished artifact never shows in the Debrief | Assembly step has no computed `:::brief-output` to bind the payload into a value slot | Add `:::brief-output { source: "computed" }` exposed on the producing step — see [A task succeeds but its result never appears](#a-task-succeeds-but-its-result-never-appears) |
| Step stops partway; later tasks in the step never run; the step won't retry | A task set `signals.unsafe = true` (a deliberate veto) | The step ends in the terminal `unsafe` phase (non-retryable, unlike an ordinary failure which retries). Check the task-attempt event's `signals.reason` for why — see [A task vetoes the step](#a-task-vetoes-the-step-signalsunsafe--true) |

---

## A task vetoes the step (`signals.unsafe = true`)

A task can **veto** its step by returning `signals.unsafe = true` (optionally with a `signals.reason`). This is different from an ordinary failure:

- The step ends in the **terminal `unsafe` phase** and **does not retry** — re-dispatching will not re-run it. (An ordinary task failure, by contrast, retries via re-dispatch.)
- Because a step is an **ordered, fail-together** unit, any **later tasks in the same step do not run**.
- A computed `:::brief-output` in that step **never publishes** — computed outputs publish only when the whole step completes successfully. This is why a safety/validation task belongs **in the same step, after the producer**: a veto there keeps the unsafe value from ever going live.

To diagnose: `GET /api/dispatches/{id}/task_attempts` → find the vetoing task's attempt event and read `signals.reason`. If the veto is unexpected, fix the condition in the task script that sets `signals.unsafe`.

---

## A task succeeds but its result never appears

**Symptom.** The journey runs all the way through. The final "assembly" step — the
one that stitches the operator's picks into the finished artifact — *succeeds*.
Yet the operator's Debrief sits in a working/waiting state and the finished
artifact never shows. It reads as "stuck" or "taking forever" even though nothing
is actually running.

**This is almost never an LLM hang.** Rule that out first:

- `GET /api/dispatches/{id}/task_attempts` → the assembly task's phase is `Ok`
  (not `Running` / `Failed`). If it is `Ok`, the model finished and the problem is
  *downstream of execution*.
- Ignore any `running` you see on a task-attempt **row** in the database. That
  `status` column is an insert-time placeholder the runtime never overwrites; the
  **effective** phase is the latest attempt event, which the task-attempts
  endpoint already resolves for you. Diagnose from the endpoint's phase, never
  from the raw row.

**Root cause: a task payload is not a journey value.** A task returning
`{ payload, signals }` does **not** put that payload into the journey. A value
becomes part of the journey state — and therefore visible to the operator and to
`GET /api/journeys/{id}/steps` — only when it is **bound to a value slot**. There
are exactly two ways a slot gets bound:

| Binding | Authored by | Binds when |
|---|---|---|
| Operator pick | `:::brief-input` (`producer: "operator"`) | the operator submits the control |
| Computed output | `:::brief-output { source: "computed" }` (`producer: "computed"`) | once the producing step coheres (all its tasks succeed, no `unsafe` veto) — the runtime extracts `source_path` from the step payload |

If your assembly step has **no computed `:::brief-output`**, the assembled payload
lands only in the dispatch task-attempt event log. The journey never sees it and
the UI has no deliverable to render. The task did the work — *the guide never
asked for the result.*

There are two shapes of this bug, told apart by what
`GET /api/journeys/{id}/steps` reports for the assembly step:

- **No computed slot listed at all.** You never declared an output. By the binding
  rules the stage may even report `done: true` — its only exposed slots are the
  operator picks, and those *are* bound — but nothing captured or rendered the
  result, so the chapter still looks unfinished. *This is the common one for
  "assembly" stages.*
- **A computed slot listed with `bound: false`.** You declared the output, but the
  runtime couldn't produce it, so the stage is genuinely stuck at `done: false`
  (the done-rollup counts computed outputs — see the
  [start-vs-done asymmetry](./value-slot-overlays.md#computed-outputs-are-produced-by-a-step-not-bound-by-an-operator)).
  Read `computed_outputs[].outcome` on the task-attempt for the reason:
  `source_path_missing` (your `source_path` matches no key in the payload) or
  `type_mismatch` (the extracted value's shape ≠ the slot's `kind`).

**The fix.** Declare a value slot for the deliverable and produce it with a
computed output that is **named**, **listed in its brief's `units`**, and whose
slot is **exposed on the producing step** (all three required):

```
# the assembly task returns { payload: { title, closer, body_sections, ... }, signals: {} }

:::value_slot { name: "assembled_story", kind: "json" }
:::

:::brief-output { name: "assembled-story-output", source: "computed",
                  slot: "assembled_story", source_path: "." }
:::

:::brief { name: "assemble-brief", units: ["assembled-story-output"] }
:::

:::step { name: "Assemble the story", task: "assemble", expose: ["assembled_story"] }
:::
```

- **Name the output and list that name in its `:::brief`'s `units`** — the
  brief→output association is by name (exactly like inputs). An unnamed output is
  dropped at parse; one absent from every `units` never materializes.
- **Expose the output's slot on its producing step** — the runtime only binds a
  computed output on a step that exposes its slot (see
  [Computed outputs are produced by a step](./value-slot-overlays.md#computed-outputs-are-produced-by-a-step-not-bound-by-an-operator)).
- `source_path: "."` captures the **whole** payload — use it when the step produces
  a *single* computed output. To surface individual fields instead, declare a slot
  per field and point each output at a JSON pointer: `source_path: "/title"`,
  `source_path: "/closer"`, `source_path: "/body_sections"`.
- Match `kind` to the value: a structured object is `kind: "json"`; a single
  string field is `kind: "text"`.
- Re-ingest after editing. Adding an output forks a new guide version, so
  **in-flight journeys keep their old shape** — start a fresh journey to see the
  change.

**Binding is not display.** A computed `:::brief-output` *binds* the value and
*gates* the stage's `done` rollup — it does **not** render anything on its own. If
you bind the whole structured payload (`source_path: "."`) the only thing that can
show it is the runtime's raw task-payload fallback, which dumps a `json` object
field-by-field (the "I get an outline, not a story" symptom). To present a clean
deliverable: lift the prose field into the slot (have the task return
`{ "story": "…" }`, bind with `source_path: "/story"`) **and** add a deliverable
`:::brief-markdown` unit that interpolates the slot:

```markdown
:::brief-markdown { name: "assemble-deliverable", interpolates: ["assembled_story"] }
## Your finished story

$assembled_story
:::
# …then list "assemble-deliverable" in the brief's units alongside the output.
```

The `$slot` is substituted server-side with the journey's bound value when the
brief-form is served, and the unit renders in the post-completion result view. See
the dapple-guides README, "Displaying the deliverable."

**Rule of thumb.** Every task whose output the operator is meant to *see* must end
at a computed `:::brief-output` exposed on the step that produces it (to bind it and
reach `done`), **plus** a deliverable `:::brief-markdown` to render it. If the result
has no slot to land in, it does not exist as far as the journey is concerned.

---

## Validate Before Ingest

Before running a full ingest, use dry-run mode to validate your guide without writing to the database.

### SDK Dry Run

```rust
use dapple_sdk::{DappleClient, types::IngestParams};

let client = DappleClient::with_api_token("https://api.example.com", "dpl_token");
let response = client.guides().ingest_from_dir(
    &std::path::Path::new("./my-guide"),
    IngestParams { dry_run: true, source_path: None },
).await?;
```

### curl Dry Run

```bash
tar czh -f my-guide.tar.gz -C my-guide/ .
curl -X POST "https://api.example.com/api/guides/ingest?dry_run=true" \
  -H "Authorization: Bearer dpl_token" \
  -F "file=@my-guide.tar.gz"
```

### What to Check

The dry-run response includes:
- `valid` -- Whether the guide parsed without errors
- `errors` -- Fatal issues that would prevent ingest
- `warnings` -- Non-fatal issues (unnamed blocks, missing Fluent keys)
- `summary` -- Entity counts (chapters, blocks, assets, derived objects)

Run dry-run validation in your CI pipeline to catch issues before deployment.

---

## Verify Derived Objects

After a successful ingest, confirm that derived entities were created correctly:

```rust
// Verify guide exists
let guide = client.guides().get_by_slug("my-guide").await?;

// Verify tasks
let tasks = client.tasks().list_by_slugs(
    vec!["task-name".into()],
    None,
).await?;

// Verify playbook
let playbooks = client.playbooks().list_by_slugs(
    vec!["my-guide".into()],
    None,
).await?;
```

---

## Common Ingest Errors

| Error | Cause | Fix |
|-------|-------|-----|
| "No 'file' field found in multipart request" | Wrong multipart field name | Use `file` as the field name |
| "Tarball exceeds maximum size" | Over 50 MB | Remove unnecessary assets or split guides |
| "Tarball contains symlinks" | Symlinks in archive | Use `tar czh` (dereference) when creating |
| "No guide.toml found" | Missing or misplaced guide.toml | Ensure guide.toml is at the tarball root or in a single top-level directory |
| "Failed to parse guide" | Syntax error in guide.toml, SUMMARY.md, or chapter | Check the error message for file and line |
| "invalid semver version" | Bad version in guide.toml | Use valid semver: "1.0.0", "0.1.0-dev" |
| "unknown deprecation level" | Invalid deprecation level | Use: deprecated, retired, or quarantined |
| "Guide patch failed" | Wrong path_object_id in PATCH URL | Verify the UUID matches an existing guide path |
