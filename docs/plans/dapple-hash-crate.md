# Extract `dapple-hash` Crate

**Status:** Draft (amended)
**Created:** 2026-02-28
**Companion plan:** `~/src/dapple/dapple-suite/docs/plans/content-hash-pinning.md` (client-side changes)

---

## Context

Content-addressable hashing is a core architectural property of dapple-prompt. The hash functions live in `dapple-data::hash`, and the `ContentHash` type is defined in `dapple-data::lib`. While the hash module itself is pure (no sqlx dependency), it lives inside a crate that depends on sqlx, postgres, and the full database model layer. This makes the hash functions inaccessible to crates that shouldn't depend on the data layer — specifically `dapple-sdk`, which is the REST client shipped to third-party consumers.

The SDK already creates tarballs via `create_tarball_from_dir(".", dir)` and sends them to the API, which computes the content hash server-side and returns it in the `IngestGuideResponse`. This is the correct flow: **the server is the authority for content hashes**.

However, there are use cases where local hash computation is valuable:

1. **Pre-flight validation**: Before sending a tarball to the API, check if the content hash matches what's already pinned — skip the network round-trip entirely
2. **Offline status**: Show `canonical` vs `modified` status without hitting the API
3. **CI/CD pipelines**: Validate guide content hasn't changed without requiring API access

For these cases, the hash algorithm must be available outside `dapple-data` without pulling in sqlx/postgres.

---

## Current State

| Component | Location | Dependencies |
|-----------|----------|-------------|
| `hash_guide_directory()` | `dapple-data/src/hash.rs` | `sha1`, `hex`, `serde_json`, `BTreeMap` |
| `is_excluded_path()` | `dapple-data/src/hash.rs` | none (pure string matching) |
| `sha1_hex()` | `dapple-data/src/hash.rs` | `sha1`, `hex` |
| `ContentHash` type | `dapple-data/src/lib.rs` | `serde`, `schemars` |
| Entity-specific hashers | `dapple-data/src/hash.rs` | `sha1`, `hex`, `serde_json` |
| Tarball creation | `dapple-sdk/src/resource/guides.rs` | `tar`, `flate2` |
| Server-side computation | `dapple-prompt-api/src/handler/guide_ingest.rs:288` | `dapple_data::hash` |

### Path prefix convention

The SDK creates tarballs with `archive.append_dir_all(".", dir)`, producing paths like `./guide.toml`, `./chapter1.md`. The server extracts these paths directly from tar entries and feeds them into `hash_guide_directory`. The `./` prefix is canonical — any local hash computation must use the same prefix.

---

## Design: `dapple-hash` Crate

A minimal crate containing only the deterministic hash functions. No database, no network. Depends on `serde_json` for `canonical_json()` and entity hashers that take `serde_json::Value` parameters.

### Crate Contents

```
crates/dapple-hash/
├── Cargo.toml
└── src/
    └── lib.rs
```

### What moves from `dapple-data`

| Function | Source | Moves to `dapple-hash`? | Notes |
|----------|--------|------------------------|-------|
| `ContentHash` type | `dapple-data/src/lib.rs:69-97` | Yes | Core type + `Default`, `Display`, `Serialize`, `Deserialize`, `JsonSchema` impls |
| `sha1_hex()` | `dapple-data/src/hash.rs` | Yes | Pure function |
| `is_excluded_path()` | `dapple-data/src/hash.rs` | Yes | Pure string matching |
| `hash_guide_directory()` | `dapple-data/src/hash.rs` | Yes | Only needs `sha1`, `hex`, `BTreeMap` |
| `normalize_text()` | `dapple-data/src/hash.rs` | Yes | Pure string transform |
| `canonical_json()` | `dapple-data/src/hash.rs` | Yes | Needs `serde_json` |
| `hash_task()`, `hash_step()`, etc. | `dapple-data/src/hash.rs` | Yes | Entity hashers for Merkle tree |
| `NUL` constant | `dapple-data/src/hash.rs` | Yes | Separator byte |

### Dependencies

```toml
[package]
name = "dapple-hash"
version = "0.1.0"
edition = "2024"

[dependencies]
sha1 = "0.10"
hex = "0.4"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Must match dapple-data's schemars version (currently 0.9.x).
# Using a mismatched major version would make ContentHash's JsonSchema
# impl incompatible with dapple-data's derive expectations.
schemars = { version = "0.9", optional = true }

[features]
default = []
schema = ["dep:schemars"]
```

### Consumer dependencies

```
dapple-data    → depends on dapple-hash (replaces inline hash module)
dapple-sdk     → depends on dapple-hash (optional feature: "hash")
dapple-suite   → gets dapple-hash transitively via dapple-sdk
```

### `dapple-data` changes

Two files change:

**`dapple-data/src/lib.rs`** — Remove the `ContentHash` struct definition (lines 69-97: struct, `new`, `short`, `as_str`, `Default`, `Display` impls). Replace with a re-export:

```rust
pub use dapple_hash::ContentHash;
```

**`dapple-data/src/hash.rs`** — Replace the entire module body with a re-export:

```rust
pub use dapple_hash::*;
```

All existing call sites (`dapple_data::hash::hash_task(...)`, `dapple_data::ContentHash::new(...)`, etc.) continue to work unchanged because both `lib.rs` and `hash.rs` re-export from the same source.

**`dapple-data/Cargo.toml`** — Add dependency, enable `schema` feature (since dapple-data uses `JsonSchema` on `ContentHash`), remove now-transitive `sha1` and `hex`:

```toml
dapple-hash = { path = "../dapple-hash", features = ["schema"] }
# sha1 and hex can be removed — now transitive via dapple-hash
```

### `dapple-sdk` changes

Add optional dependency:

```toml
[features]
hash = ["dep:dapple-hash"]

[dependencies.dapple-hash]
path = "../dapple-hash"
optional = true
```

Re-export for consumers:

```rust
#[cfg(feature = "hash")]
pub use dapple_hash;
```

---

## Testing

### Golden vector test

Both `dapple-data` and `dapple-sdk` (when `hash` feature is enabled) should run the same golden vector test to prove they produce identical hashes:

```rust
#[test]
fn test_guide_hash_golden_vector() {
    let mut files = BTreeMap::new();
    files.insert("./guide.toml".to_string(), b"title = \"Test Guide\"\n".to_vec());
    files.insert("./chapter1.md".to_string(), b"# Chapter 1\n\nHello world.\n".to_vec());
    files.insert("./locales/en.ftl".to_string(), b"greeting = Hello\n".to_vec());

    let hash = hash_guide_directory(&files);
    assert_eq!(hash.as_str(), "<computed-golden-value>");
}
```

The golden value is computed once, hardcoded, and asserted in both crates. If either implementation diverges, the test fails.

### Exclusion test with `./` prefix

```rust
#[test]
fn test_exclusion_with_dot_prefix() {
    // Paths in tarballs have ./ prefix; is_excluded_path works on the suffix
    assert!(!is_excluded_path("guide.toml"));
    assert!(is_excluded_path(".git/config"));
    assert!(is_excluded_path(".DS_Store"));
}
```

---

## Migration Steps

1. Create `crates/dapple-hash/` with `ContentHash` (moved from `dapple-data/src/lib.rs`), all pure hash functions (moved from `dapple-data/src/hash.rs`), and `is_excluded_path`
2. Add golden vector tests in `dapple-hash`
3. Update `dapple-data/Cargo.toml` to depend on `dapple-hash` (with `schema` feature); remove direct `sha1` and `hex` deps
4. Replace `dapple-data/src/lib.rs` `ContentHash` definition with `pub use dapple_hash::ContentHash;`
5. Replace `dapple-data/src/hash.rs` internals with `pub use dapple_hash::*;`
6. Verify all `dapple-data` and `dapple-prompt-api` tests still pass (same functions, just re-exported)
7. Add `hash` feature to `dapple-sdk/Cargo.toml`
8. Enable `hash` feature in `dapple-suite`'s dependency on `dapple-sdk`
9. Verify golden vector test passes in both `dapple-hash` and via `dapple-sdk`

---

## Design Consideration: Feature-Gating Entity Hashers

The SDK's primary use case is `hash_guide_directory()` + `is_excluded_path()`. The entity hashers (`hash_task`, `hash_step`, `hash_fluent_message`, etc.) are server-internal concerns that require `serde_json::Value`.

A possible refinement: split into a base set (always present) and entity hashers (behind a feature):

- **Always present:** `ContentHash`, `sha1_hex`, `normalize_text`, `is_excluded_path`, `hash_guide_directory`, `NUL`
- **`entities` feature (default):** `canonical_json`, `hash_task`, `hash_step`, `hash_stage`, `hash_playbook`, and all other entity hashers (pulls in `serde_json`)

This would let the SDK depend on `dapple-hash` without `serde_json`. However, `serde_json` is already a transitive dependency of most Rust projects, so the savings may not justify the complexity. **Decision: start without the feature gate; add it later if SDK binary size becomes a concern.**

---

## Out of Scope

- Changing how the API computes or stores hashes (no server changes)
- Changing the tarball format or path prefix convention
- Adding hash computation to the REST API response (it's already there)
- Client-side usage of `dapple-hash` — that's covered in the companion plan
