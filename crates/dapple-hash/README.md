# dapple-hash

Content-addressable hashing for dapple versioned objects.

## Overview

`dapple-hash` provides deterministic SHA-1 hashing used by the content-addressable versioning system. When a versioned object is created or patched, its content hash is computed and compared against the latest version — if they match, the operation short-circuits and returns the existing version (deduplication).

## ContentHash Type

```rust
use dapple_hash::ContentHash;

let hash = ContentHash::new("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d".to_string());
assert_eq!(hash.short(), "aaf4c61d");  // 8-char prefix (like git)
assert_eq!(hash.as_str().len(), 40);   // Full 40-char hex
```

`ContentHash` derives `Serialize`, `Deserialize`, `Clone`, `Eq`, `Hash`, `Debug`. With the `schema` feature, it also derives `JsonSchema` for OpenAPI generation.

## Utility Functions

| Function | Purpose |
|----------|---------|
| `normalize_text(s)` | CRLF→LF, trim whitespace — ensures identical logical content hashes identically |
| `canonical_json(value)` | Deterministic JSON via BTreeMap key sorting — `{"b":1,"a":2}` and `{"a":2,"b":1}` produce the same string |
| `sha1_hex(data)` | SHA-1 hex digest from raw bytes |
| `is_excluded_path(path)` | Filters `.git/`, `__MACOSX/`, `.DS_Store`, `.gitignore`, `.swp`, `.swo` from guide directory hashing |

## Entity Hash Functions

Each entity type has a dedicated hash function that defines which fields contribute to its content identity:

| Function | Inputs | Pattern |
|----------|--------|---------|
| `hash_task` | description, function, function_input | Simple concatenation |
| `hash_step` | description, flow_control, task_content_hashes | Merkle (ordered child task hashes; N=1 byte-identical to the legacy single-task hash) |
| `hash_stage` | description, flow_control, step_content_hashes | Merkle (ordered child hashes) |
| `hash_playbook` | description, stage_content_hashes | Merkle (ordered child hashes) |
| `hash_guide_directory` | `BTreeMap<String, Vec<u8>>` | File-by-file hash of directory contents |
| `hash_prompt` | locale_bodies, engine_bodies | Sorted locale + engine body pairs (Merkle-equivalent: matches the prompt's linked fluent message hash) |
| `hash_fluent_message` | locale_bodies, engine_bodies | Sorted locale + engine body pairs |
| `hash_category` | description | Single field |
| `hash_hat` | description | Single field |
| `hash_brief` | content (JSON) | Canonical JSON |
| `hash_brief_group` | description, brief_content_hashes | Merkle (ordered child brief content_hashes) |
| `hash_brief_pack` | description, group_content_hashes | Merkle (ordered child brief_group content_hashes) |
| `hash_value_slot` | title, description, constraints | Multi-field |
| `hash_asset` | raw bytes | SHA-1 of binary data |
| `hash_directive` | directive_type, config | Multi-field |
| `hash_inference_profile` | locale_bodies, locale_attributes | Sorted locale bodies + per-locale fluent attributes (system-prompt, temperature, etc.) |

### Merkle Pattern

Playbooks, stages, and steps form a Merkle tree: each parent's hash includes its children's content hashes. Any change to a descendant propagates upward, enabling cascade dedup detection.

```
playbook_hash = hash(description, [stage_hash_1, stage_hash_2, ...])
  stage_hash  = hash(description, flow_control, [step_hash_1, step_hash_2, ...])
    step_hash = hash(description, flow_control, [task_hash_1, task_hash_2, ...])
```

## Dependencies

Minimal: `sha1`, `hex`, `serde`, `serde_json`. Optional `schemars` behind the `schema` feature flag.

## Features

| Feature | Purpose |
|---------|---------|
| `schema` | Enables `JsonSchema` derive on `ContentHash` (required by `dapple-types` and `dapple-data`) |

## Dependency Graph

```
dapple-hash (this crate)
    ^
    |
dapple-types (re-exports ContentHash)
    ^
    |
dapple-data (uses hash functions + ContentHash)
```
