# dapple-types

Lightweight, consumer-facing DTO types for the Dapple Prompt API.

## Overview

`dapple-types` is the single source-of-truth for all public data transfer objects (DTOs) shared between the server (`dapple-data`, `dapple-prompt-api`) and clients (`dapple-sdk`). It contains **only** serialization-ready types with no database, async, or infrastructure dependencies.

## Why This Crate Exists

The server's internal data layer (`dapple-data`) carries heavy dependencies — sqlx, redis, pgvector, tokio — that SDK consumers don't need. Previously, the SDK auto-generated types from a stale OpenAPI spec, causing deserialization drift. `dapple-types` solves both problems: it provides canonical types that both sides import directly.

## What Lives Here

### Core Types (`src/lib.rs`)

| Type | Purpose |
|------|---------|
| `DateTime` | Wrapper around `time::OffsetDateTime` with RFC 3339 serde and JsonSchema |
| `FilterOptions` | Pagination params (`page`, `limit`) with `sql_values()` helper |
| `JsonObject` | Schema helper for freeform JSON fields (OpenAPI `additionalProperties`) |
| `DataTransferObject` | Trait contract for all public DTOs (`fn id(&self) -> &Uuid`) |
| `ContentHash` | Re-exported from `dapple-hash` — 40-char hex SHA-1 for content-addressable versioning |

### Reference Types (`src/reference.rs`)

| Type | Purpose |
|------|---------|
| `ObjectReference` | Trait for lightweight UUID references to other entities |
| `CategoryReference`, `StepReference`, `TaskReference`, etc. | Typed UUID wrappers for FK references in DTOs |
| `*PathReference` variants | Typed references to versioned path objects |

### Model DTOs (`src/model/`)

Every consumer-facing entity has its DTOs here, one file per entity domain:

| File | Key Types |
|------|-----------|
| `guides.rs` | `Guide`, `GuideRef`, `GuideCard`, `GuideChapter`, `GuideBlock`, `GuideVersion` |
| `playbooks.rs` | `Playbook`, `PlaybookStage`, `PlaybookStep`, `PlaybookTask` |
| `journeys.rs` | `Journey`, `JourneyRef`, `JourneyBriefContext`, `JourneyBrief` |
| `briefs.rs` | `Brief`, `BriefPath`, `BriefOutput` |
| `brief_units.rs` | `BriefUnit`, `BriefUnitSubtype`, all input/media/notice variants |
| `brief_groups.rs` | `BriefGroup`, `BriefGroupPath`, `BriefGroupReference` |
| `brief_packs.rs` | `BriefPack`, `BriefPackPath` |
| `steps.rs` | `Step`, `StepTask` |
| `tasks.rs` | `Task`, `TaskPromptRef` |
| `stages.rs` | `Stage` |
| `categories.rs` | `Category` |
| `hats.rs` | `Hat`, `HatCategory` |
| `assets.rs` | `Asset`, `AssetPath` |
| `directives.rs` | `Directive` |
| `dispatches.rs` | `Dispatch`, `DispatchStage`, `DispatchStep`, `DispatchTask` |
| `dispatch_intent_bindings.rs` | `DispatchIntentBinding`, all binding item types |
| `dispatch_task_attempts.rs` | `DispatchTaskAttempt` |
| `fluent_messages.rs` | `FluentMessage`, `FluentMessageBody`, `FluentMessageEngineBody` |
| `prompts.rs` | `Prompt`, `PromptPath` |
| `value_slots.rs` | `ValueSlot`, `ValueSlotPath` |
| `inference_profile.rs` | `InferenceProfile`, `InferenceProfileConfig` |
| `versioned_objects.rs` | `EntityType`, `DeprecationLevel`, `VersionedObject`, `DeprecationEvent` |
| `canonical.rs` | `CanonicalReleasePin` |
| `user_details.rs` | `UserDetail` |
| `user_favorite_guides.rs` | `UserFavoriteGuide` |
| `user_favorite_journeys.rs` | `UserFavoriteJourney` |

## What Does NOT Live Here

Everything database-related stays in `dapple-data`:

- Internal models: `*Model`, `Flat*Model`, `Inserted*Model`, `Insert*Args`
- `From<Model> for DTO` conversion impls
- Query functions, SQL assets, error types
- Database traits: `RelationalObject`, `PrincipalObject`, `Keyed`, etc.
- Macros: `impl_dto_id!`, `impl_principal_traits!`, etc.
- Caching, Redis, pgvector, connection pooling

## Dependencies

```
dapple-hash (sha1, hex, serde)
    ^
    |
dapple-types (serde, schemars, uuid, time, dapple-hash)
    ^          ^
    |          |
dapple-data   dapple-sdk
```

Only six dependencies: `serde`, `serde_json`, `uuid`, `time`, `schemars`, `dapple-hash`. No sqlx, no tokio, no async.

## Usage

### From dapple-data (server-side)

`dapple-data` re-exports everything, so existing server code works unchanged:

```rust
// These all resolve through dapple-data's re-exports
use dapple_data::DateTime;
use dapple_data::model::guides::Guide;
use dapple_data::reference::CategoryReference;
```

### From dapple-sdk (client-side)

```rust
use dapple_types::DateTime;
use dapple_types::model::guides::Guide;
use dapple_types::reference::CategoryReference;
```

### Implementing DataTransferObject

DTOs implement `DataTransferObject` via macros in `dapple-data`:

```rust
// In dapple-data (not here):
impl_dto_id!(Guide);
```

## Adding a New DTO

1. Define the struct in the appropriate `src/model/{entity}.rs` file
2. Derive `Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize`
3. Use `DateTime` for timestamps, `ContentHash` for hashes, `Uuid` for IDs
4. Re-export from `dapple-data/src/model/{entity}.rs`
5. Add `DataTransferObject` impl via macro in `dapple-data`
