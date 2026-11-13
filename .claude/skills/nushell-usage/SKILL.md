---
name: nushell-usage
description: Essential patterns, idioms, and parser gotchas for writing Nushell code, with a section dedicated to traps that have repeatedly broken `.nu` scripts in the dapple-prompt repo. Use whenever you're writing or modifying any `.nu` file, debugging parse errors in nushell, generating Nushell scripts, or reviewing nu code — even if the user doesn't explicitly invoke the skill. Especially useful when editing files under `assets/nu/` in dapple-prompt.
version_target: "0.100+"
---

# Nushell Usage Patterns

This skill captures both general Nushell idioms and a small set of parser gotchas that have repeatedly cost real time in the dapple-prompt repo. Read the **Project-specific gotchas** section first when working on `.nu` files in this codebase — those patterns have each cost at least one real debugging session.

The general patterns and the broader pitfalls/best-practices follow.

---

## Project-specific gotchas (read first when editing `assets/nu/**/*.nu`)

These patterns have shown up multiple times in dapple-prompt and each one took a debug round-trip to identify. Scan for them before committing nu code.

### 1. Parens in `$"..."` interpolated strings are evaluated as expressions

Inside an interpolated string `$"..."`, `(...)` is parsed as a nushell expression — it is **not** treated as literal text. Bare-word phrases that look like English are parsed as command invocations and fail at runtime with `Command 'no' not found` (or similar).

```nu
# WRONG — `(idempotent)` evaluates to a call to a command named `idempotent`
log info $"registering token (idempotent)"

# WRONG — `(no action needed)` parses as `no` command with two args
log debug $"path is auto-mounted (no action needed)"

# RIGHT — square brackets are literal in interpolated strings
log info $"registering token [idempotent]"
log debug $"path is auto-mounted [no action needed]"

# ALSO RIGHT — escape the parens
log info $"registering token \(idempotent\)"
```

Real nu expressions inside `$"..."` are correct and intentional and should not be touched:
- `$"($var)"` — variable reference
- `$"(random uuid)"` — calling the `random uuid` builtin
- `$"(date now | format date '%Y%m%d')"` — pipeline
- `$"item ($i + 1) of ($total)"` — arithmetic

The trap is specifically when the parens contain prose that *looks* like English. When you're writing log lines, status messages, or any human-readable text inside an interpolated string, prefer square brackets for parentheticals or escape the parens.

### 2. Bareword `flag=...` syntax breaks for both single AND double quotes

`kubectl get foo -o jsonpath='{.something}'` and `kubectl get foo -o jsonpath="{.something}"` are both bash-style shell-quote-removal syntax. In nushell, `jsonpath=` is parsed as a bareword followed by `=`, and the quoted string after it is a separate token. The two halves don't recombine the way bash would, and the resulting kubectl arg list is wrong — sometimes silently (kubectl returns empty / wrong-shaped output) and sometimes loudly (parse error on the embedded special chars).

```nu
# WRONG (single quotes) — bareword followed by raw string
kubectl get foo -o jsonpath='{.status.conditions[?(@.type=="Complete")].status}'

# WRONG (double quotes) — same trap, even though it looks innocuous
kubectl get foo -o jsonpath="{.spec.template.spec.volumes}"

# RIGHT — wrap the ENTIRE `jsonpath=…` expression as one quoted string
kubectl get foo -o 'jsonpath={.status.conditions[?(@.type=="Complete")].status}'
kubectl get foo -o "jsonpath={.spec.template.spec.volumes}"
```

The fully-quoted form is required: the `jsonpath=` prefix and the value must be inside the same pair of quotes. Use double quotes by default; switch to single quotes only when the path contains characters that double-quoted nushell strings interpret (`(...)`, `\n`, `$x`).

The silent-failure mode is particularly nasty: kubectl returns a JSON-encoded *string* of the value instead of the value itself, so `from json` fails downstream with confusing "trailing characters" errors. If you see `from json` blowing up on what looks like valid kubectl output, suspect this trap.

### 3. `$in` inside a block does NOT see the previous statement's result

`$in` refers to the block's *piped input*, not the value produced by the previous statement inside the block. This trips up patterns where you want to "run a command, then check its result" inside a try/do/etc.

```nu
# WRONG — `$in.exit_code` is reading the block's piped input (nothing),
# not the result of the `security ... | complete` from the line above.
# The catch swallows the resulting error and returns false silently.
let in_keychain = (try {
    ^security find-certificate ... | complete
    $in.exit_code == 0
} catch { false })

# RIGHT — bind the | complete record to a let so the next line can read it
let result = (do { ^security find-certificate ... } | complete)
let in_keychain = ($result.exit_code == 0)
```

This bug is silently bad — the check returns false on every run, the script "fixes" something that didn't need fixing (often re-prompting for sudo or re-doing expensive work), and nobody notices because the script keeps working.

If you find yourself writing two statements inside a block where the second one references the first, **always bind the first to a `let`** (or a record destructure) and refer to the named binding.

### 4. Variadic `[...args]` swallows flags meant for the inner command

A custom `def` with `[...args]` captures positional arguments, but nushell still tries to parse `--flag` and `-x` tokens as flags of the def itself before they reach the inner command. Result: `Unknown flag` errors when callers pass dash-prefixed args.

```nu
# WRONG — `--classic` is consumed by vm_run's flag parser
export def vm_run [...args] {
    ^orb run -m vm ...$args
}
orb vm_run sudo snap install microk8s --classic   # ERROR: unknown flag

# RIGHT — accept the command as a list literal
export def vm_run [cmd: list<string>] {
    ^orb run -m vm ...$cmd
}
orb vm_run [sudo snap install microk8s --classic]  # works

# ALSO RIGHT — `--wrapped` def for transparent flag forwarding
export def --wrapped kubectl [...args] {
    ^kubectl ...$args
}
kubectl get pods --namespace foo   # works, all flags pass through
```

**When to use which:**
- `--wrapped` when the def is a drop-in replacement for an external CLI and every flag should pass through verbatim.
- `list<string>` parameter when callers can be expected to wrap the command in `[...]` and the def has its OWN flags it needs to parse.

### 5. `describe` names a list of records `table<...>` — never gate success on "list"

A non-empty list of records describes as `table<...>`; only an EMPTY list (or a
heterogeneous one) says `list<...>`. So a shape gate that asks "is this the success
list?" with `starts-with "list"` classifies every real result as the failure shape and
passes only the empty case — the inverse of what it means to check.

```nu
[{a: 1} {a: 2}] | describe   # table<a: int>   ← NOT list<...>
[{a: 1}] | describe          # table<a: int>
[] | describe                # list<any>
```

```nu
# WRONG — every non-empty success falls into the error branch; an empty stub passes,
# so a smoke test that fakes the return never catches it
let pulled = (edgar manager $cik)
if (not (($pulled | describe) | str starts-with "list")) {
    error make { msg: "no such filer" }   # fires on a fully successful pull
}

# RIGHT — branch on the exceptional shape (records), let table/list fall through
if (($pulled | describe) | str starts-with "record") {
    # non-resolution record: read $pulled.outcome? and branch
}
```

This shipped in mind-the-13f (2026-08-02): `edgar manager` resolved the filer, fetched
every filing, returned the success table — and the script reported "matches no filer of
Form 13F on EDGAR" for every valid manager. Two rounds of infra archaeology (pod
restarts, env wiring) happened before anyone questioned the one-line gate. If a command
returns "table on success, record on failure", gate on the record side.

### Bonus: paste-ready commands shouldn't wrap with `nu -c`

When suggesting commands to someone working in dapple-prompt, write them as bare nu (paste-ready into their interactive nushell):

```
# RIGHT — paste-ready
kubectl get pods -n dapple-sandbox

# WRONG — they're already in nushell, this spawns a child nu
nu -c 'kubectl get pods -n dapple-sandbox'
```

The `nu -c '...'` form is only correct when the user is starting from zsh/bash. Inside an interactive nu (the dapple-prompt default), it wastes a process and often loses overlay env state.

---

## Smaller-but-still-recurring gotchas

These are bash idioms that don't map cleanly to nushell. Easy to fix once you know.

| Bash | Nushell |
|------|---------|
| `cmd 2>&1` | `cmd out+err> file` or `cmd out+err>\| ...` |
| `cmd 2>/dev/null` | `cmd err> /dev/null` |
| `mkdir -p path/to/dir` | `mkdir path/to/dir` (nushell creates parents by default) |
| `ls -la` | nushell's builtin: `ls -a -l` (not combined `-la`) — or `^ls -la` for the external |
| `open file.yaml` (auto-parses YAML) | works because of the extension |
| `open ~/.kube/config` (no extension) | returns a string — use `open --raw \| from yaml` |
| `$nu.home-dir` (≥0.80) vs `$nu.home-path` (older) | use the version-compat snippet below |

```nu
# Cross-version home directory
let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
```

---

## General patterns

The rest of this skill is general Nushell idioms. Skim once; refer back when needed.

### Pipeline input vs parameters

Pipeline input (`$in`) is NOT interchangeable with function parameters.

```nu
# WRONG — treats $in as first parameter
def my-func [list: list, value: any] {
    $list | append $value
}

# RIGHT — declares pipeline signature
def my-func [value: any]: list -> list {
    $in | append $value
}

# Usage
[1 2 3] | my-func 4  # Works correctly
my-func [1 2 3] 4    # ERROR! my-func doesn't take positional params
```

Pipeline input can be **lazily evaluated** (streaming); parameters are **eagerly evaluated** (loaded into memory). Different calling conventions entirely.

### Type signatures

```nu
# No pipeline input
def func [x: int] { ... }                    # (x) -> output

# Pipeline input only
def func []: string -> int { ... }           # string | func -> int

# Both pipeline and parameters
def func [x: int]: string -> int { ... }     # string | func x -> int

# Generic pipeline
def func []: any -> any { ... }              # works with any input type
```

### Working with lists

```nu
# Filter with index
$list | enumerate | where {|e| $e.index > 5 and $e.item.some-bool-field}

# Reduce with explicit accumulator
$list | reduce --fold 0 {|item, acc| $acc + $item.value}
```

### Working with records

```nu
# Create
{name: "Alice", age: 30}

# Merge (right-biased)
$rec1 | merge $rec2

# Update field via closure
$rec | update name {|r| $"Dr. ($r.name)"}

# Insert based on existing fields
{x: 1, y: 2} | insert z {|r| $r.x + $r.y}

# Upsert (update or insert)
$rec | upsert count {|r| ($r.count? | default 0) + 1}

# Reject / select
$rec | reject password secret_key
$rec | select name age email
```

### Working with tables

Tables are lists of records.

```nu
let table = [
    {name: "Alice", age: 30}
    {name: "Bob", age: 25}
]

$table | where age > 25
$table | insert retired {|row| $row.age > 65}
$table | rename -c {age: years}
$table | group-by status --to-table
$table | transpose name data   # rows ↔ columns
```

### Conditionals return values

```nu
let result = if $condition { "yes" } else { "no" }

let result = match $value {
    0 => "zero"
    1..10 => "small"
    _ => "large"
}
```

### Null safety

```nu
$record.field?                    # Returns null if missing
$record.field? | default "N/A"    # Provide fallback
if ($record.field? != null) { ... }
```

### Error handling

```nu
# Try-catch
try {
    dangerous-operation
} catch {|err|
    print $"Error: ($err.msg)"
}

# Returning errors
def my-func [] {
    if $condition {
        error make { msg: "Something went wrong" }
    } else {
        "success"
    }
}

# Detailed error info for EXTERNAL commands
let result = (fallible-external-command | complete)
if $result.exit_code != 0 {
    print $"Error: ($result.stderr)"
}
```

### Closures and scoping

```nu
# Closures capture environment
let multiplier = 10
let double_and_add = {|x| ($x * 2) + $multiplier}
5 | do $double_and_add  # 20

# Outer mutable variables CANNOT be captured in closures
mut sum = 0
[1 2 3] | each {|x| $sum = $sum + $x}  # WON'T COMPILE

# Use reduce instead
let sum = [1 2 3] | reduce {|x, acc| $acc + $x}
```

### Iteration

```nu
$list | each {|item| $item * 2}                 # transform
$list | par-each {|item| expensive-op $item}    # parallel
$list | where {|x| $x > 10}                     # filter (closure form)
$table | where size > 100                       # filter (row condition: $it.size > 100)

# reduce: accumulate
$list | reduce --fold 0 {|item, acc| $acc + $item}

# for loop (imperative)
for item in $list { print $item }
```

### Strings

```nu
$"Hello ($name)!"
$"Sum: (1 + 2)"           # "Sum: 3"

"a,b,c" | split row ","
["a", "b"] | str join ", "

"hello123" | parse --regex '(?P<word>\w+)(?P<num>\d+)'
```

### Glob patterns (file matching)

```nu
glob *.rs
glob **/*.rs                 # recursive
glob **/*.{rs,toml}          # multiple extensions
```

Prefer `glob` over `find` or `ls` for file searches in nu — better pattern support, faster.

### Modules

```nu
module my_module {
    export def public-func [] { ... }
    def private-func [] { ... }            # not exported
    export const MY_CONST = 42
}

use my_module *
use my_module [public-func MY_CONST]
use lib/helpers.nu *
```

---

## Row conditions vs closures

Many commands accept either a **row condition** (short syntax with auto-`$it` expansion) or a **closure** (full flexibility).

```nu
# Row condition — implicit $it.size, $it.name
$table | where size > 100
$table | where name =~ "test"

# Closure — explicit $row or $in
$table | where {|row| $row.size > 100}
$table | where {$in.size > 100}
```

Limitations of row conditions:
- Cannot be stored in variables.
- Only field access on the LHS auto-expands. Subexpressions need explicit `$it`:

  ```nu
  ls | where ($it.name | str downcase) =~ readme
  ```

Closures can be stored and reused:

```nu
let big_files = {|row| $row.size > 1mb}
ls | where $big_files
```

Use row conditions for simple field comparisons; closures for complex logic or reusable conditions.

---

## Common pitfalls (general nushell)

### `each` on single records

```nu
# WRONG — runs once on the whole record
let record = {a: 1, b: 2}
$record | each {|field| print $field}

# RIGHT — items, transpose
$record | items {|key, val| print $"($key): ($val)"}
$record | transpose key val | each {|row| ... }
```

### Pipe vs call ambiguity

```nu
$list | my-func arg1 arg2   # $list piped, arg1 & arg2 as params
my-func $list arg1 arg2     # $list as positional (if signature allows)
```

### Optional fields error if missing

```nu
$record.missing            # ERROR
$record.missing?           # null
$record.missing? | default "N/A"
```

### Empty collections

```nu
if ($list | is-empty) { ... }
$list | default -e $val_if_empty
```

---

## Best practices

1. **Use type signatures** — catch errors early and document intent.
2. **Prefer pipelines** over imperative loops where possible — more idiomatic and composable.
3. **Comment with `#`** — both inline and as doc comments above declarations.
4. **Export selectively** — don't pollute namespace.
5. **Use `default`** — handle null gracefully.
6. **Validate inputs early** — type/range checks at function start.
7. **Return consistent types** — don't mix null and values unexpectedly.
8. **Use modules** — organize related functions.
9. **Build pipelines incrementally** — test each stage step-by-step.
10. **Prefix externals with `^`** — `^grep` instead of `grep`. Makes it clear it's not a nushell command and avoids ambiguity. **Nushell builtins always have precedence**: e.g. `find` is NOT the Unix `find` tool — use `^find`.
11. **Use external tools when they're faster** — `^rg` for large file searches, `^jq` for huge JSON. Don't try to do everything in pure nu.

---

## Debugging techniques

```nu
# Print intermediate values without interrupting the pipeline
$data | each {|x| print $x; $x}

# Inspect type
$value | describe

# Drop into the debugger (when available)
debug

# Time a block
timeit { expensive-command }
```

---

## Deeper references

For advanced patterns and a complete type-system guide, two reference files are bundled with this skill. Read them on demand when the question goes beyond the patterns above.

- **[references/advanced-patterns.md](references/advanced-patterns.md)** — performance optimization, lazy evaluation and streaming, closure semantics, memory-efficient patterns. Useful when a pipeline is slower or more memory-hungry than expected, or when you need streaming behavior across `each`/`reduce`.
- **[references/type-system.md](references/type-system.md)** — complete type-system reference, conversions, generics, type guards. Useful when type signatures don't behave as expected or when designing generic helper functions.

## External resources

- [Official Nushell Book](https://www.nushell.sh/book/)
- [Nushell Cookbook](https://www.nushell.sh/cookbook/)
- [Type Signatures](https://www.nushell.sh/book/custom_commands.html#type-signatures)
- [Modules](https://www.nushell.sh/book/modules.html)

---

## Attribution

The general-patterns sections are adapted from [YPares/agent-skills/nushell-usage](https://github.com/YPares/agent-skills/tree/main/nushell-usage). The project-specific gotchas come from debugging real failures in dapple-prompt during the multipass→OrbStack migration and earlier work.
