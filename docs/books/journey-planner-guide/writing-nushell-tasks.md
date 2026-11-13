# Writing Nushell Tasks

> **Audience:** :wrench: Prompt Engineers · :robot: Agents

Recommended conventions for scripts that become Tasks.

In the guide authoring workflow, Nushell tasks are defined as inline `:::nushell` blocks within guide chapters. The content of a `:::nushell` block becomes the script body that the workflow engine executes. This chapter covers the contract and best practices for writing these scripts.

---

## 1. The `{ payload, signals }` Contract

Every Nushell script executed by the workflow engine **must** return a record with two keys:

- `payload` — The task output (any JSON-serializable value)
- `signals` — Control-plane metadata (a record/map)

```nushell
let result = $in | process-data
{ payload: $result, signals: { completed: true, items_processed: ($result | length) } }
```

This contract is enforced by the `StepEngine`. Scripts that do not return this shape will fail at runtime.

## 2. Inputs

- Accept inputs via the pipeline (`$in`) as JSON for complex structures
- Document expected input shape in a short header comment

```nushell
# TASK: collect-personal-data
# Inputs: JSON on stdin with keys: name, email, work_history
```

## 3. Exit Codes and Errors

- Exit 0 for success, non-zero for failure
- On failure, write a short error message to stderr and prefer machine-readable JSON in `error.json` in OUT_DIR

```nushell
# On error
{
    message: "Failed to parse input JSON"
    code: 400
} | save --force $"($env.OUT_DIR)/error.json"
exit 1
```

## 4. Idempotency

- Tasks should be safe to re-run
- If they produce an artifact that already exists, either overwrite deterministically or write to a versioned filename

## 5. Logging

- Write human-friendly logs to stdout
- Write machine-friendly logs (JSON lines) to `$env.OUT_DIR/log.jsonl` when running in CI or under Playbook orchestration

```nushell
# Append to log
{
    timestamp: (date now | format date "%Y-%m-%dT%H:%M:%S")
    level: "info"
    message: "Processing started"
} | to json --raw | save --append $"($env.OUT_DIR)/log.jsonl"
```

## 6. Tasks that feed or consume keep-N picks

A **keep-N** (`cardinality: "multi"`) pick lets the operator curate a *subset* of generated candidates. Two task roles touch it. See [Value-Slot Overlays → Multi-select picks](./value-slot-overlays.md#multi-select-picks-keep-n--curate-a-subset) for the full authoring recipe; this is the task contract.

**Generate task → candidate source.** A task that produces the choice tiles must return them under a key your computed `:::brief-output { source_path: "/choices" }` extracts. The canonical shape is a `choices` array of `{ label, description, value }`:

```nushell
# TASK: extract-suppliers  — feeds the `supplier_candidates` (kind:json) slot
let drafts = $in.text | extract-supplier-drafts
{
    payload: {
        choices: ($drafts | each {|d| { label: $d.name, description: $d.why, value: $d.name } })
    },
    signals: { extracted: ($drafts | length) }
}
```

The operator sees `label`/`description` on the tiles and the chosen `value`s become the binding items, so `value` must match the **pick** slot's `kind` (text option → `kind: "text"`).

**Refine task ← the kept subset.** The consuming task of a `multi` slot receives the kept items as an **array** under `$in.bindings.<slot>` (a `single` slot delivers a scalar — unchanged):

```nushell
# TASK: provision-suppliers — kept_suppliers is cardinality:"multi"
let kept = $in.bindings.kept_suppliers      # => ["Recheio", "talho do João"]
let rows = $kept | each {|name| provision-one $name }
{ payload: { provisioned: $rows }, signals: { count: ($kept | length) } }
```

**Computed multi-output (no operator).** A task can also fan out *without* a curation beat: a `:::brief-output { source: "computed" }` whose `source_path` yields an **array** into a `multi` slot binds one item per element. Same payload shape — just point `source_path` at the array your task emits.

---

## Inline `:::nushell` Blocks

In guide authoring, Nushell tasks are embedded directly in markdown chapters using fenced directive blocks:

```markdown
:::nushell { name: "validate-input", version: "1.0.0", tags: [agent] }
let input = $in
# ... validation logic ...
{ payload: $input, signals: { validated: true } }
:::
```

### Attributes

| Attribute | Required | Description |
|-----------|----------|-------------|
| `name` | Yes | The task's slug, used for deduplication across ingests |
| `version` | Yes | Semver string enabling idempotent ingest (same slug + version = reuse existing task) |
| `tags` | No | Audience filtering tags (e.g., `[agent]`, `[agent, coder]`) |

### How It Works

- The `name` attribute becomes the task's slug in the database. If a task with the same slug and version already exists, the ingest pipeline reuses it rather than creating a duplicate.
- The `version` attribute enables idempotent ingest. Bumping the version creates a new task version; keeping it the same reuses the existing one.
- Tags (e.g., `tags: [agent]`) control audience filtering via render profiles. Blocks tagged `[agent]` only appear in agent-facing renders.
- The content between `:::nushell` and the closing `:::` becomes the script body that the workflow engine executes. It must conform to the `{ payload, signals }` return contract.

### Example: Multi-step Task Pipeline

```markdown
:::nushell { name: "collect-personal-data", version: "1.0.0" }
let input = $in
# Validate required fields
if ($input.name? | is-empty) {
    error make { msg: "Missing required field: name" }
}
{ payload: $input, signals: { fields_collected: ($input | columns | length) } }
:::

:::nushell { name: "format-resume", version: "1.0.0", tags: [agent] }
let data = $in
let markdown = $"# ($data.name)\n\n($data.work_history)"
{ payload: { markdown: $markdown }, signals: { formatted: true } }
:::
```

---

## Standalone Testing

When developing and testing tasks outside the guide ingest pipeline, you can use the `OUT_DIR` and `IN_ARTIFACT` environment model.

### Environment Variables

| Variable | Description |
|----------|-------------|
| `OUT_DIR` | Directory where the task writes output artifacts |
| `IN_ARTIFACT` | Path to an upstream artifact (for chained tasks) |

### Standalone Task Skeleton

```nushell
#!/usr/bin/env nu

# TASK: collect-personal-data
# Inputs: expects $env.OUT_DIR, accepts JSON on stdin with keys: name, email, work_history
# Outputs: writes input.json and artifact.json to OUT_DIR

def main [] {
    let out_dir = $env.OUT_DIR? | default "./out"
    mkdir $out_dir

    # Read JSON from stdin
    let input = $in | from json

    # Validate required fields
    if ($input.name? | is-empty) {
        {
            message: "Missing required field: name"
            code: 400
        } | save --force $"($out_dir)/error.json"
        exit 1
    }

    # Write input to file
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

    print $"Artifact written to ($out_dir)/artifact.json"
}
```

---

## Notes

- Prefer simple, focused scripts; keep external dependencies minimal
- Document required binaries in the Task header comment
- **Privacy:** If processing resumes, mask PII when storing logs or example data in shared Brief Packs
- When migrating standalone scripts to guide `:::nushell` blocks, replace file I/O patterns (`OUT_DIR`, `artifact.json`) with the `{ payload, signals }` return contract
