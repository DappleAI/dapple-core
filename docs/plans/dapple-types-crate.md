# Extract `dapple-types` Crate from `dapple-data`

**Status:** Reviewed
**Created:** 2026-03-01
**Dependencies:** `dapple-hash` crate (already extracted, see `dapple-hash-crate.md`)

---

## Executive Summary

The `dapple-sdk` currently auto-generates its types from a stale `openapi/internal-api.json` file via typify at build time. The generated types have drifted from reality: the server now returns `id`/`path_id` but the generated SDK structs still expect `object_id`/`path_object_id`, and fields like `content_hash` on Guide are missing entirely. This causes deserialization failures in consumers like the TUI.

The real source-of-truth types live in `dapple-data/src/model/*.rs` as pure `serde`/`schemars` structs. But `dapple-data` is too heavy for the SDK to depend on — it pulls in sqlx, redis, pgvector, tokio, async-trait, and the full query/cache layer.

This plan extracts a lightweight `dapple-types` crate containing **only** the public DTO structs and their supporting types (DateTime, FilterOptions, references). After extraction:

1. `dapple-data` depends on `dapple-types` and re-exports its types
2. `dapple-sdk` depends on `dapple-types` directly, eliminating the stale OpenAPI codegen
3. `dapple-prompt-api` schema types import `FilterOptions` from `dapple-types` (transitively via `dapple-data`)
4. The `openapi/internal-api.json` file, `build.rs`, and typify build dependency are removed from `dapple-sdk`

---

## Context and Background

### The Drift Problem

The SDK's type generation pipeline is:

```
Server (dapple-prompt-api)
  -> aide generates OpenAPI spec
  -> developer manually copies spec to openapi/internal-api.json
  -> build.rs runs typify codegen
  -> generated.rs included at compile time
```

This pipeline has a manual copy step that has fallen behind. The server's DTO types have evolved (field renames: `object_id` -> `id`, `path_object_id` -> `path_id`; new fields: `content_hash`, `deprecation_level`) but the checked-in spec has not been updated.

### The Weight Problem

The true source-of-truth types live in `dapple-data/src/model/*.rs`. Each model file contains two categories:

1. **Public DTOs** — `#[derive(JsonSchema, Serialize, Deserialize)]`, use `DateTime` and `ContentHash`, intended for API consumers
2. **Internal models** — `FlatXxxModel`, `InsertedXxxModel`, `XxxModel`, `InsertXxxArgs` — used by sqlx queries, reference `OffsetDateTime`, have `From<Row>` impls

The DTOs depend only on: `serde`, `schemars`, `uuid`, `time`, `serde_json`, `dapple_hash::ContentHash`. But they live in a crate that transitively requires sqlx, redis, pgvector, tokio, etc.

### The dapple-hash Precedent

`dapple-hash` was successfully extracted using this exact pattern: move pure types/functions to a new lightweight crate, have `dapple-data` re-export them, let `dapple-sdk` depend on the new crate directly.

---

## What Goes Into dapple-types

### Guiding Principle

A type belongs in `dapple-types` if and only if:

1. It appears in API responses (DTO) or API request query parameters, **AND**
2. It depends only on `serde`, `schemars`, `uuid`, `time`, `serde_json`, `dapple-hash`, or `rust_decimal` — **no sqlx, no database traits, no async**

### From `dapple-data/src/lib.rs`

| Type | Description | Dependencies |
|------|-------------|-------------|
| `DateTime` | Custom wrapper around `time::OffsetDateTime` with RFC3339 serde + JsonSchema | `time`, `serde`, `schemars` |
| `FilterOptions` | Pagination params (`page`, `limit`) with `sql_values()` helper | `serde`, `schemars` |
| `JsonObject` | Schema helper for freeform JSON fields | `schemars` |
| `ContentHash` re-export | Already in `dapple-hash` | (transitive) |

### From `dapple-data/src/traits.rs` — DTO Trait

| Type | Description | Dependencies |
|------|-------------|-------------|
| `DataTransferObject` | Trait providing `fn id(&self) -> &Uuid` on all public DTOs | `uuid` |

This trait is the public contract for DTOs. It must move to `dapple-types` because all DTOs implement it (via `impl_dto_id!` and `impl_principal_traits!` macros in `dapple-data`). All other traits in `traits.rs` (`RelationalObject`, `PrincipalObject`, `Keyed`, `InsertedModel`, `FlatRelationalObject`, etc.) are internal-only and stay in `dapple-data`.

### From `dapple-data/src/model/*.rs` — Public DTOs

Each model file has clearly marked "Public DTOs" sections. Move all structs with `#[derive(JsonSchema, Serialize, Deserialize)]`:

- **Guides**: `GuideRef`, `Guide`, `GuideCard`, `GuideAssetRef`, `GuideChapter`, `GuideBlock`, `GuideVersion`, `GuideCardCarouselItem`, `GuideSharedRef`
- **Playbooks**: `Playbook`, `PlaybookStage`, `PlaybookStep`, `PlaybookTask`
- **Journeys**: `JourneyRef`, `BriefPackRef`, `InferenceProfileRef`, `Journey`, `JourneyBriefContext`, `JourneyBrief`
- **Briefs**: `Brief`, `BriefPath`, `BriefOutput`
- **Brief Units**: `BriefUnit`, `BriefUnitSubtype`, all input/media/notice structs
- **Brief Groups**: `BriefGroup`, `BriefGroupPath`, `BriefGroupReference`
- **Brief Packs**: `BriefPack`, `BriefPackPath`
- **Steps**: `Step`, `StepTask`
- **Tasks**: `Task`, `TaskPromptRef`
- **Stages**: `Stage`
- **Categories**: `Category`
- **Hats**: `Hat`
- **Assets**: `Asset`, `AssetPath`
- **Directives**: `Directive`
- **Dispatches**: `Dispatch`, `DispatchStage`, `DispatchStep`, `DispatchTask`
- **Dispatch Intent Bindings**: `DispatchIntentBinding`, all binding item types
- **Dispatch Task Attempts**: `DispatchTaskAttempt`
- **Fluent Messages**: `FluentMessage`, `FluentMessageBody`, `FluentMessageEngineBody`
- **Prompts**: `Prompt`, `PromptPath`
- **Value Slots**: `ValueSlot`, `ValueSlotPath`
- **Inference Profiles**: `InferenceProfile`, `InferenceProfileConfig`
- **User Details**: `UserDetail`
- **Versioned Objects**: `EntityType`, `DeprecationLevel`, `DeprecationLevelKey`, `DeprecationEvent`
- **Canonical**: `CanonicalReleasePin`
- **User Favorites**: `UserFavoriteGuide`, `UserFavoriteJourney`

### From `dapple-data/src/reference.rs` — Consumer-Facing References

Only the types with `#[derive(JsonSchema)]`:
- `ObjectReference` trait, all `*Reference` and `*PathReference` types

### What Stays in dapple-data

Everything database-related:
- `DbPool`, `DataModel`, `MergeOrdered`, `InsertColumnArgs`, `Keyed` traits
- All internal traits: `RelationalObject`, `PrincipalObject`, `InsertedModel`, `FlatRelationalObject`, `VersionedRelationalObject`, `PathRelationalObject`, `UnifiedVersionedObject`, `HasPathDetails`, `Depreciable`, `OwnedDependent`, `SharedDependent`
- All `*Model`, `Flat*Model`, `Inserted*Model`, `Insert*Args` structs
- All `From<Model> for DTO` impls
- All macros (`impl_keyed!`, `impl_versioned!`, `impl_insert_column_args!`, `validate_insert_args!`, `impl_principal_traits!`, `impl_dto_id!`, `impl_flat_to_parent!`) — macros stay in `dapple-data` and reference `DataTransferObject` via `dapple-data`'s re-export
- `error`, `cache`, `publisher`, `redis_cache`, `redis_publisher`, `query` modules
- Internal model references (`CategoryModelReference`, etc.)

### What Stays in dapple-sdk

SDK-specific request/response types:
- `IngestParams`, `IngestGuideResponse`, `SharedIngestParams`, `SharedIngestResponse`, `DeleteQueryParams`
- `ApiTokenInfo`, `CreateApiTokenSchema`, `ListApiTokensResponse`, `CreateApiTokenResponse`, `RevokeApiTokenResponse`
- `AllQuery`, `ByObjectIdsQuery`, `ByJourneyQuery`, `GetDispatchTaskAttemptSetSchema`
- `VersionedObject`, `GetVersionedObjectSetSchema`

---

## Crate Structure

```
crates/dapple-types/
├── Cargo.toml
└── src/
    ├── lib.rs              # DateTime, FilterOptions, JsonObject, DataTransferObject, re-export ContentHash
    ├── reference.rs        # ObjectReference trait + consumer-facing references
    └── model/
        ├── mod.rs
        ├── guides.rs
        ├── playbooks.rs
        ├── journeys.rs
        ├── briefs.rs
        ├── brief_units.rs
        ├── brief_groups.rs
        ├── brief_packs.rs
        ├── steps.rs
        ├── tasks.rs
        ├── stages.rs
        ├── categories.rs
        ├── hats.rs
        ├── assets.rs
        ├── directives.rs
        ├── dispatches.rs
        ├── dispatch_intent_bindings.rs
        ├── dispatch_task_attempts.rs
        ├── fluent_messages.rs
        ├── prompts.rs
        ├── value_slots.rs
        ├── inference_profiles.rs
        ├── user_details.rs
        ├── versioned_objects.rs
        ├── canonical.rs
        └── user_favorites.rs
```

Submodule hierarchies are **flattened** (e.g. `PlaybookStage` goes into `playbooks.rs` alongside `Playbook`).

---

## Dependencies

```toml
[package]
name = "dapple-types"
version = "0.1.0"
edition = "2024"
description = "Public DTO types for the Dapple Prompt API"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
uuid = { version = "1", features = ["serde", "v4", "v7"] }
time = { version = "0.3", features = ["serde"] }
schemars = { version = "0.9", features = ["uuid1"] }
dapple-hash = { path = "../dapple-hash", features = ["schema"] }
rust_decimal = { version = "1.38", features = ["serde"] }
```

---

## Changes to Existing Crates

### dapple-data

**Cargo.toml**: Add `dapple-types = { path = "../dapple-types" }`

**src/lib.rs**: Remove `DateTime`, `FilterOptions`, `JsonObject` definitions. Replace with:
```rust
pub use dapple_types::{DateTime, FilterOptions, JsonObject, ContentHash, DataTransferObject};
```

**src/traits.rs**: Remove `DataTransferObject` trait definition. Add re-export:
```rust
pub use dapple_types::DataTransferObject;
// All other internal traits (RelationalObject, PrincipalObject, etc.) stay here
```

**src/macros.rs**: Update `impl_dto_id!` path. The macro currently uses `$crate::traits::DataTransferObject`. Since `dapple-data` re-exports `DataTransferObject` via `pub use dapple_types::DataTransferObject` in `lib.rs`, the `$crate::` path continues to resolve correctly — the re-export in `traits.rs` ensures `$crate::traits::DataTransferObject` still works. **No macro changes required** as long as the re-export is in place. Same applies to `impl_principal_traits!`.

**src/reference.rs**: Remove consumer-facing reference types. Replace with:
```rust
pub use dapple_types::reference::*;
// Internal model references stay here
```

**src/model/*.rs**: For each model file, remove DTO definitions and add re-exports:
```rust
pub use dapple_types::model::guides::{Guide, GuideRef, GuideCard, ...};
// Internal models, fold logic, From impls, insert args stay
```

### dapple-sdk

**Delete**: `openapi/internal-api.json`, `build.rs`, `src/generated.rs`

**Remove from Cargo.toml**: Entire `[build-dependencies]` section (typify, schemars 0.8, regex).

**Add to Cargo.toml**: `dapple-types = { path = "../dapple-types" }`

**src/lib.rs**: Remove `mod generated;`

**src/types.rs**: Replace `pub use crate::generated::*;` with:
```rust
pub use dapple_types::*;
pub use dapple_types::model::*;
```

SDK-specific manual types remain in `types.rs`.

### dapple-prompt-api

**No code changes needed.** It imports DTOs from `dapple_data::model::*` which re-exports from `dapple-types`.

One consideration: `schemars::schema_id()` uses `module_path!()` which changes from `dapple_data::` to `dapple_types::`. The implementation agent should diff the OpenAPI spec before/after to verify.

---

## Migration Strategy

### Phase 1: Create dapple-types with lib.rs types and DataTransferObject trait
1. Create crate, move `DateTime`, `FilterOptions`, `JsonObject`, `DataTransferObject`
2. Re-export in `dapple-data` (`lib.rs` and `traits.rs`), verify `impl_dto_id!` / `impl_principal_traits!` macros still resolve
3. Verify compilation

### Phase 2: Move reference types
3. Move `ObjectReference` trait and consumer-facing references
4. Re-export in `dapple-data`, verify compilation

### Phase 3: Move model DTOs (one entity at a time)
5. Start with `Guide` (motivating case — has the drift)
6. Then `Playbook`, `Journey`, `Brief`, `BriefUnit`
7. Then all remaining entities
8. Verify `cargo build --workspace` and `cargo test -p dapple-data` at each step

### Phase 4: Update dapple-sdk
9. Add `dapple-types` dependency, delete codegen artifacts
10. Update `types.rs`, define local query schemas
11. Verify `cargo build -p dapple-sdk`

### Phase 5: End-to-end verification
12. Full workspace build + clippy
13. Diff OpenAPI spec before/after
14. Smoke test TUI against live API

### Phase 6: Documentation
15. Create `crates/dapple-types/README.md` — full inventory of what lives in the crate, usage examples, dependency graph
16. Create `crates/dapple-hash/README.md` — ContentHash type, hash function inventory, Merkle pattern docs
17. Update `crates/dapple-data/README.md` — note that DTOs now live in dapple-types, update stale code examples, fix module organization diagram

---

## Risks and Open Questions

### Risk: Schema ID Changes
`module_path!()` in `JsonSchema` impls changes from `dapple_data::` to `dapple_types::`. Low risk — aide uses type names, not full paths. **Mitigate by diffing OpenAPI spec.**

### Risk: Uuid Version Mismatch
`dapple-types` and `dapple-data` (via sqlx) must use the same `uuid` crate version. **Mitigate by workspace-level uuid pin.**

### Note: rust_decimal Dependency
`rust_decimal` is listed in the proposed dependencies. The implementation agent should verify which DTOs actually use `Decimal` fields — if none do, omit the dependency to keep the crate lighter.

### Resolved: chrono vs time
SDK should switch entirely to `dapple_types::DateTime` (backed by `time`). Consistency across the type boundary is the whole point of this extraction.

### Resolved: Favorite Response Types
`AddFavoriteGuideResponse` etc. stay in `dapple-prompt-api/src/schema/`. These are API-specific response wrappers, not domain DTOs.

---

## Dependency Graph After Migration

```
dapple-hash (pure: sha1, hex, serde)
    ^
    |
dapple-types (pure: serde, schemars, uuid, time, dapple-hash)
    ^          ^
    |          |
dapple-data   dapple-sdk
(sqlx, redis, (reqwest, serde_qs,
 pgvector,     url, async-trait)
 dapple-types)
    ^
    |
dapple-prompt-api
(axum, aide, dapple-data)
```

The key achievement: `dapple-sdk` no longer needs a build step, no longer depends on a stale JSON file, and always has types that match the server's actual API responses.
