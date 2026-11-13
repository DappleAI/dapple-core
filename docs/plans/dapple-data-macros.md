# dapple-data Declarative Macros Architecture Plan

**Status:** Draft
**Created:** 2026-02-18
**Last Updated:** 2026-02-18
**Target Version:** 0.2.0
**Dependencies:** None (purely internal refactoring)

---

## 1. Executive Summary

The `dapple-data` crate has grown to approximately 21,000 lines across 39 model files and 36 query files. While the codebase follows strict, well-documented conventions, it has accumulated significant mechanical boilerplate -- identical patterns repeated verbatim across dozens of entity types.

This plan introduces 7 declarative macros (`macro_rules!`) that eliminate approximately 8,100 lines of repetitive code. The macros are divided into two tiers:

- **Tier 1 (pure mechanical, zero variation):** 4 macros targeting trait impls and validation boilerplate
- **Tier 2 (mostly mechanical, some variation):** 3 macros targeting query wrappers, model conversions, and flat-to-parent transforms

All macros are declarative (`macro_rules!`). No proc macro crate is needed because every pattern can be captured with Rust's token-tree matching. The macros live in a single new file `crates/dapple-data/src/macros.rs` and are consumed at call sites in existing model and query files.

**Who benefits:** Implementation agents creating new entities will write 60-70% less boilerplate per entity. Existing code becomes easier to audit because the mechanical parts collapse into one-line invocations, leaving only the genuinely unique logic visible.

---

## 2. Context and Background

### Current State

Every principal entity in `dapple-data` follows the same structural pattern:

1. **InsertedXxxModel** -- struct returned by `INSERT...RETURNING`
2. **FlatXxxModel** -- struct from `SELECT` with `LEFT JOIN`s, with `fold_models()` method
3. **XxxModel** -- internal model with children as `Vec<Child>`
4. **Xxx** -- public DTO (no `id: i64`)
5. **InsertXxxArgs** -- column-wise insert arguments

For each entity, the following trait implementations are required:

| Trait | On | Lines per entity |
|---|---|---|
| `Keyed` | InsertedXxxModel | 8 |
| `Keyed` | XxxModel | 8 |
| `traits::RelationalObject` | InsertedXxxModel | 8 |
| `traits::RelationalObject` | XxxModel | 8 |
| `traits::InsertedModel` | InsertedXxxModel | 1 |
| `traits::PrincipalObject` | XxxModel | 4 |
| `traits::VersionedRelationalObject` | XxxModel | 10 |
| `traits::UnifiedVersionedObject` | XxxModel | 10 |
| `traits::Depreciable` | XxxModel | 4 |
| `traits::FlatRelationalObject` | FlatXxxModel | 10-20 |
| `traits::DataTransferObject` | Xxx | 5 |
| `InsertColumnArgs` | InsertXxxArgs | 8-12 |

Multiplied across ~20 unified-versioned entities, this produces thousands of lines of code with zero meaningful variation.

### Problems Addressed

1. **Copy-paste risk** -- Manual repetition of identical patterns invites subtle errors (wrong field name, missing `as i16` cast)
2. **Onboarding friction** -- New entities require writing 100+ lines of boilerplate before any unique logic
3. **Audit burden** -- Reviewers must scan identical blocks to verify they match the pattern
4. **Inconsistency** -- Some files use `<Self as crate::Keyed>::id(self)` for `RelationalObject`, others inline `&self.id` directly

### Design Principles

- **Declarative only** -- `macro_rules!` avoids adding a proc macro crate, compile-time dependencies, and build complexity
- **Opt-in migration** -- Existing files can adopt macros incrementally; no big-bang rewrite required
- **Zero runtime change** -- Macros expand to the exact same code that exists today
- **Grep-friendly** -- Macro names are distinctive (`impl_keyed!`, `impl_versioned!`) so `grep` still finds trait implementations

---

## 3. File Organization

### New File

```
crates/dapple-data/src/macros.rs    -- All 7 macro definitions
```

### Module Registration

In `crates/dapple-data/src/lib.rs`, add at the top (before other modules):

```rust
#[macro_use]
mod macros;
```

The `#[macro_use]` attribute makes all `#[macro_export]` macros available throughout the crate without explicit imports. This is the standard Rust pattern for crate-internal utility macros.

### Why Not Multiple Files?

All 7 macros share the same conceptual purpose (reducing model/query boilerplate) and are tightly related. Splitting them across files would scatter the documentation and make it harder for agents to find the right macro. A single 300-line file is straightforward to navigate.

---

## 4. Tier 1 Macros -- Pure Mechanical

### 4.1 `impl_keyed!` -- Keyed + RelationalObject

**Lines eliminated:** ~800 across the crate

**Pattern being replaced:**

Every model with `id: i64` and `object_id: Uuid` has identical implementations of both `Keyed` (from `lib.rs`) and `traits::RelationalObject`:

```rust
// Current: repeated for InsertedTaskModel, TaskModel, InsertedStepModel, StepModel, etc.
impl crate::Keyed for InsertedTaskModel {
    fn id(&self) -> &i64 { &self.id }
    fn object_id(&self) -> &sqlx::types::Uuid { &self.object_id }
}
impl crate::traits::RelationalObject for InsertedTaskModel {
    fn id(&self) -> &i64 { &self.id }
    fn object_id(&self) -> &sqlx::types::Uuid { &self.object_id }
}
```

**Macro definition:**

```rust
/// Implement `Keyed` and `traits::RelationalObject` for a type with `id: i64` and `object_id: Uuid`.
///
/// Usage:
///   impl_keyed!(InsertedTaskModel);
///   impl_keyed!(TaskModel);
///
/// Optionally marks the type as `InsertedModel`:
///   impl_keyed!(InsertedTaskModel, inserted);
macro_rules! impl_keyed {
    ($ty:ty) => {
        impl $crate::Keyed for $ty {
            fn id(&self) -> &i64 {
                &self.id
            }
            fn object_id(&self) -> &sqlx::types::Uuid {
                &self.object_id
            }
        }

        impl $crate::traits::RelationalObject for $ty {
            fn id(&self) -> &i64 {
                &self.id
            }
            fn object_id(&self) -> &sqlx::types::Uuid {
                &self.object_id
            }
        }
    };

    // Variant that also implements InsertedModel marker trait
    ($ty:ty, inserted) => {
        impl_keyed!($ty);
        impl $crate::traits::InsertedModel for $ty {}
    };
}
```

**Call site (tasks.rs, after trait implementations section):**

```rust
// Before: 22 lines
// After: 2 lines
impl_keyed!(InsertedTaskModel, inserted);
impl_keyed!(TaskModel);
```

**Entities affected:** All 39 model files. Most files have 2-4 types needing `impl_keyed!`.

**Variation inventory:**
- Some models (DTOs like `BriefGroup`, `Prompt`, `PromptPath`) implement `Keyed` with a `static DUMMY_ID: i64 = 0` pattern. These are **not** candidates for this macro and should remain hand-written. The macro is strictly for types that have a real `pub id: i64` field.

---

### 4.2 `impl_versioned!` -- VersionedRelationalObject + UnifiedVersionedObject + Depreciable

**Lines eliminated:** ~250 across the crate

**Pattern being replaced:**

Every unified-versioned XxxModel has identical implementations of three traits:

```rust
impl crate::traits::VersionedRelationalObject for TaskModel {
    fn version_major(&self) -> i16 { self.version_major as i16 }
    fn version_minor(&self) -> i16 { self.version_minor as i16 }
    fn version_patch(&self) -> i16 { self.version_patch as i16 }
}

impl crate::traits::UnifiedVersionedObject for TaskModel {
    fn versioned_object_id(&self) -> i64 { self.versioned_object_id }
    fn path_object_id(&self) -> &Uuid { &self.path_object_id }
    fn entity_type_key(&self) -> &str {
        crate::model::versioned_objects::entity_type_keys::TASK
    }
}

impl crate::traits::Depreciable for TaskModel {
    fn deprecation_level(&self) -> Option<&str> {
        self.deprecation_level_key.as_deref()
    }
}
```

**Macro definition:**

```rust
/// Implement `VersionedRelationalObject`, `UnifiedVersionedObject`, and `Depreciable`
/// for a unified-versioned model type.
///
/// Requires the type to have fields:
///   - `version_major: i32`, `version_minor: i32`, `version_patch: i32`
///   - `versioned_object_id: i64`
///   - `path_object_id: Uuid`
///   - `deprecation_level_key: Option<String>`
///
/// Usage:
///   impl_versioned!(TaskModel, TASK);
///   impl_versioned!(PlaybookModel, PLAYBOOK);
///
/// The second argument is the entity_type_keys constant name.
macro_rules! impl_versioned {
    ($ty:ty, $entity_key:ident) => {
        impl $crate::traits::VersionedRelationalObject for $ty {
            fn version_major(&self) -> i16 {
                self.version_major as i16
            }
            fn version_minor(&self) -> i16 {
                self.version_minor as i16
            }
            fn version_patch(&self) -> i16 {
                self.version_patch as i16
            }
        }

        impl $crate::traits::UnifiedVersionedObject for $ty {
            fn versioned_object_id(&self) -> i64 {
                self.versioned_object_id
            }
            fn path_object_id(&self) -> &sqlx::types::Uuid {
                &self.path_object_id
            }
            fn entity_type_key(&self) -> &str {
                $crate::model::versioned_objects::entity_type_keys::$entity_key
            }
        }

        impl $crate::traits::Depreciable for $ty {
            fn deprecation_level(&self) -> Option<&str> {
                self.deprecation_level_key.as_deref()
            }
        }
    };
}
```

**Call site (tasks.rs):**

```rust
// Before: 18 lines
// After: 1 line
impl_versioned!(TaskModel, TASK);
```

**Entities affected:** TaskModel, StepModel, StageModel, PlaybookModel, CategoryModel, HatModel, PromptModel, BriefGroupModel, BriefPackModel, BriefModel (brief_units), AssetModel, FluentMessageModel, DirectiveModel, ValueSlotModel, InferenceProfileModel, GuideModel (~16 entities).

---

### 4.3 `impl_insert_column_args!` -- InsertColumnArgs

**Lines eliminated:** ~300 across the crate (plus eliminates copy-paste risk in column name strings)

**Pattern being replaced:**

```rust
impl InsertColumnArgs for InsertTaskArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("names", self.names.len()),
            ("descriptions", self.descriptions.len()),
            ("functions", self.functions.len()),
            ("function_inputs", self.function_inputs.len()),
        ])
    }
}
```

**Macro definition:**

```rust
/// Implement `InsertColumnArgs` for a struct by listing its column fields.
///
/// Usage:
///   impl_insert_column_args!(InsertTaskArgs, [names, descriptions, functions, function_inputs]);
///   impl_insert_column_args!(InsertStageArgs, [names, descriptions]);
macro_rules! impl_insert_column_args {
    ($ty:ty, [$($field:ident),+ $(,)?]) => {
        impl $crate::InsertColumnArgs for $ty {
            fn column_counts(&self) -> Result<Vec<(&str, usize)>, $crate::error::DataError> {
                Ok(vec![
                    $( (stringify!($field), self.$field.len()), )+
                ])
            }
        }
    };
}
```

**Call site (tasks.rs):**

```rust
// Before: 9 lines
// After: 1 line
impl_insert_column_args!(InsertTaskArgs, [names, descriptions, functions, function_inputs]);
```

**Entities affected:** Every `InsertXxxArgs` struct across all model files (~25 structs).

---

### 4.4 `validate_insert_args!` -- Insert Args Validation Functions

**Lines eliminated:** ~360 across the crate

**Pattern being replaced:**

Every query file that has an insert function defines an identical validation helper:

```rust
fn validate_insert_task_args(objs: &[InsertTaskArgs]) -> Result<(), DataError> {
    let mut expected = None;
    for obj in objs.iter() {
        if expected.is_none() {
            expected = Some(obj.column_count()?);
        }
        obj.validate_column_counts(expected)?;
    }
    Ok(())
}
```

**Macro definition:**

```rust
/// Generate a validation function for insert args slices.
///
/// The generated function validates that all items in the slice have consistent column counts.
///
/// Usage:
///   validate_insert_args!(validate_insert_task_args, InsertTaskArgs);
///
/// Generates:
///   fn validate_insert_task_args(objs: &[InsertTaskArgs]) -> Result<(), DataError> { ... }
macro_rules! validate_insert_args {
    ($fn_name:ident, $args_ty:ty) => {
        fn $fn_name(objs: &[$args_ty]) -> Result<(), $crate::error::DataError> {
            let mut expected = None;
            for obj in objs.iter() {
                if expected.is_none() {
                    expected = Some(obj.column_count()?);
                }
                obj.validate_column_counts(expected)?;
            }
            Ok(())
        }
    };
}
```

**Call site (query/tasks.rs):**

```rust
// Before: 11 lines
// After: 1 line
validate_insert_args!(validate_insert_task_args, InsertTaskArgs);
```

**Entities affected:** Every query file with an `insert_xxx_models` function (~15 files).

---

## 5. Tier 2 Macros -- Mostly Mechanical

### 5.1 `impl_principal_traits!` -- Full Trait Bundle for Unified Versioned Principals

**Lines eliminated:** ~1,400 (combines patterns 1, 2, and parts of patterns 6 and 7)

This is a **compound macro** that bundles the full trait implementation suite for a unified-versioned principal. Most entities in the codebase are unified-versioned principals and need the exact same set of trait implementations. Rather than calling 4-5 separate macros, one invocation handles the whole block.

**Pattern being replaced:**

The entire "Trait Implementations" section at the bottom of model files like `tasks.rs`, `stages.rs`, `categories.rs`, etc.:

```rust
// tasks.rs - Trait Implementations section (62 lines)
impl crate::traits::InsertedModel for InsertedTaskModel {}
impl crate::Keyed for InsertedTaskModel { ... }
impl crate::traits::RelationalObject for InsertedTaskModel { ... }
impl crate::traits::RelationalObject for TaskModel { ... }
impl crate::traits::PrincipalObject for TaskModel { ... }
impl crate::traits::VersionedRelationalObject for TaskModel { ... }
impl crate::traits::UnifiedVersionedObject for TaskModel { ... }
impl crate::traits::Depreciable for TaskModel { ... }
impl crate::traits::FlatRelationalObject for FlatTaskModel { ... }
impl crate::traits::DataTransferObject for Task { ... }
```

**Macro definition:**

```rust
/// Implement the full suite of traits for a unified-versioned principal object.
///
/// This macro bundles: Keyed, RelationalObject, InsertedModel, PrincipalObject,
/// VersionedRelationalObject, UnifiedVersionedObject, Depreciable, and DataTransferObject.
///
/// Usage:
///   impl_principal_traits! {
///       entity_key: TASK,
///       inserted: InsertedTaskModel,
///       model: TaskModel,
///       dto: Task,
///       display_field: name,
///   }
///
/// For entities without a `name` field for display (rare), omit `display_field`:
///   impl_principal_traits! {
///       entity_key: ASSET,
///       inserted: InsertedAssetModel,
///       model: AssetModel,
///       dto: Asset,
///   }
macro_rules! impl_principal_traits {
    (
        entity_key: $entity_key:ident,
        inserted: $inserted:ty,
        model: $model:ty,
        dto: $dto:ty,
        display_field: $display:ident $(,)?
    ) => {
        impl_keyed!($inserted, inserted);
        impl_keyed!($model);

        impl $crate::traits::PrincipalObject for $model {
            fn display_name(&self) -> Option<&str> {
                Some(&self.$display)
            }
        }

        impl_versioned!($model, $entity_key);

        impl $crate::traits::DataTransferObject for $dto {
            fn object_id(&self) -> &sqlx::types::Uuid {
                &self.object_id
            }
        }
    };

    // Variant without display_field (PrincipalObject uses default None)
    (
        entity_key: $entity_key:ident,
        inserted: $inserted:ty,
        model: $model:ty,
        dto: $dto:ty $(,)?
    ) => {
        impl_keyed!($inserted, inserted);
        impl_keyed!($model);

        impl $crate::traits::PrincipalObject for $model {}

        impl_versioned!($model, $entity_key);

        impl $crate::traits::DataTransferObject for $dto {
            fn object_id(&self) -> &sqlx::types::Uuid {
                &self.object_id
            }
        }
    };
}
```

**Call site (tasks.rs) -- replaces 62 lines with 7:**

```rust
impl_principal_traits! {
    entity_key: TASK,
    inserted: InsertedTaskModel,
    model: TaskModel,
    dto: Task,
    display_field: name,
}

// TaskPromptRef DTO still done manually (it's a reference type, not a principal)
impl crate::traits::DataTransferObject for TaskPromptRef {
    fn object_id(&self) -> &sqlx::types::Uuid { &self.object_id }
}
```

**Call site (stages.rs) -- replaces 55 lines with 6:**

```rust
impl_principal_traits! {
    entity_key: STAGE,
    inserted: InsertedStageModel,
    model: StageModel,
    dto: Stage,
    display_field: name,
}
```

**Entities affected:** TaskModel, StepModel, StageModel, PlaybookModel, CategoryModel, HatModel, PromptModel, BriefGroupModel, BriefPackModel, AssetModel, FluentMessageModel, DirectiveModel, ValueSlotModel, InferenceProfileModel, GuideModel (~15 entities).

**What stays manual:**
- `FlatRelationalObject` impl (handled separately by `impl_flat_to_parent!` in section 5.3)
- Extra `DataTransferObject` impls for reference types (e.g., `TaskPromptRef`, `StepTask`)
- `OwnedDependent` impls on child models (too varied to macro-ize)
- Non-versioned entities (e.g., `JourneyModel`, `DispatchModel`, `ApiTokenModel`) -- these lack the version/deprecation fields

---

### 5.2 `impl_dto_object_id!` -- DataTransferObject for DTOs

**Lines eliminated:** ~200

A lightweight single-purpose macro for the common case where a DTO only needs the `DataTransferObject` trait with its `object_id` field.

**Pattern being replaced:**

```rust
impl crate::traits::DataTransferObject for StepTask {
    fn object_id(&self) -> &sqlx::types::Uuid {
        &self.object_id
    }
}
```

**Macro definition:**

```rust
/// Implement `DataTransferObject` for a DTO type with `object_id: Uuid`.
///
/// Usage:
///   impl_dto_object_id!(StepTask);
///   impl_dto_object_id!(PlaybookStage);
///   impl_dto_object_id!(TaskPromptRef);
macro_rules! impl_dto_object_id {
    ($ty:ty) => {
        impl $crate::traits::DataTransferObject for $ty {
            fn object_id(&self) -> &sqlx::types::Uuid {
                &self.object_id
            }
        }
    };
}
```

**Call site:**

```rust
// Before: 5 lines
// After: 1 line
impl_dto_object_id!(StepTask);
```

**Entities affected:** Every DTO type in the crate (~30 types). This is used both standalone (for reference types, child DTOs) and is also embedded inside `impl_principal_traits!`.

---

### 5.3 `impl_flat_to_parent!` -- FlatRelationalObject::to_parent_inserted

**Lines eliminated:** ~750

**Pattern being replaced:**

Every `FlatXxxModel` has a `to_parent_inserted()` method that copies fields one-by-one into an `InsertedXxxModel`. The fields that get copied are always the shared fields between the Flat and Inserted models (id, object_id, versioned_object_id, path_id, path_object_id, plus entity-specific fields).

```rust
impl crate::traits::FlatRelationalObject for FlatTaskModel {
    type ParentInserted = InsertedTaskModel;
    fn to_parent_inserted(&self) -> Self::ParentInserted {
        InsertedTaskModel {
            id: self.id,
            object_id: self.object_id,
            versioned_object_id: self.versioned_object_id,
            path_id: self.path_id,
            path_object_id: self.path_object_id,
            name: self.name.clone(),
            slug: self.slug.clone(),
            version_major: self.version_major,
            version_minor: self.version_minor,
            version_patch: self.version_patch,
            description: self.description.clone(),
            function: self.function.clone(),
            function_input: self.function_input.clone(),
            created_at: self.created_at,
        }
    }
}
```

**Macro definition:**

```rust
/// Implement `FlatRelationalObject` for a flat model type.
///
/// Fields listed as `copy` are assigned directly (for Copy types like i64, Uuid, OffsetDateTime).
/// Fields listed as `clone` are `.clone()`d (for String, Vec, Option<String>, JsonValue, etc.).
///
/// Usage:
///   impl_flat_to_parent! {
///       FlatTaskModel => InsertedTaskModel {
///           copy: [id, object_id, versioned_object_id, path_id, path_object_id,
///                  version_major, version_minor, version_patch, created_at],
///           clone: [name, slug, description, function, function_input],
///       }
///   }
///
///   impl_flat_to_parent! {
///       FlatStageModel => InsertedStageModel {
///           copy: [id, object_id, versioned_object_id, path_id, path_object_id,
///                  version_major, version_minor, version_patch, created_at],
///           clone: [name, slug, description],
///       }
///   }
macro_rules! impl_flat_to_parent {
    (
        $flat:ty => $inserted:ty {
            copy: [$($copy_field:ident),* $(,)?],
            clone: [$($clone_field:ident),* $(,)?] $(,)?
        }
    ) => {
        impl $crate::traits::FlatRelationalObject for $flat {
            type ParentInserted = $inserted;

            fn to_parent_inserted(&self) -> Self::ParentInserted {
                <$inserted> {
                    $( $copy_field: self.$copy_field, )*
                    $( $clone_field: self.$clone_field.clone(), )*
                }
            }
        }
    };
}
```

**Call site (tasks.rs):**

```rust
// Before: 18 lines
// After: 6 lines
impl_flat_to_parent! {
    FlatTaskModel => InsertedTaskModel {
        copy: [id, object_id, versioned_object_id, path_id, path_object_id,
               version_major, version_minor, version_patch, created_at],
        clone: [name, slug, description, function, function_input],
    }
}
```

**Call site (stages.rs):**

```rust
impl_flat_to_parent! {
    FlatStageModel => InsertedStageModel {
        copy: [id, object_id, versioned_object_id, path_id, path_object_id,
               version_major, version_minor, version_patch, created_at],
        clone: [name, slug, description],
    }
}
```

**Call site (prompts.rs) -- note: no slug field:**

```rust
impl_flat_to_parent! {
    FlatPromptModel => InsertedPromptModel {
        copy: [id, object_id, versioned_object_id, path_id, path_object_id,
               version_major, version_minor, version_patch, created_at],
        clone: [name, description],
    }
}
```

**Entities affected:** Every entity with a `FlatXxxModel` and corresponding `InsertedXxxModel` (~16 entities).

---

## 6. Macros NOT Included (Deferred)

### 6a. From/Into Model Conversions (Pattern 6)

The `From<InsertedXxxModel> for XxxModel` and `From<XxxModel> for Xxx` conversions were originally identified as pattern 6. After analysis, these are **deferred** because:

1. **High variation** -- Each conversion has entity-specific field mappings (e.g., `deprecation_level_key: None`, `tasks: Vec::new()`, `version: Version::new(...)`)
2. **Different target shapes** -- InsertedModel->Model adds default children vecs; Model->DTO maps `deprecation_level_key` to `deprecation_level`, converts `OffsetDateTime` to `DateTime`, and constructs `Version` from three fields
3. **Macro would be nearly as verbose as the manual code** -- The field lists and transformation rules differ enough that the macro invocation would be almost as long as the original code

**Recommendation:** Revisit once 10+ entities have been migrated to `impl_principal_traits!` and the remaining manual `From` impls feel like the dominant boilerplate.

### 6b. Association Insert Wrappers (Pattern 4)

The association insert functions (e.g., `insert_journey_brief_packs`, `insert_brief_group_briefs`) follow a validate-then-query pattern but vary in:

1. **Number of column vectors** (2-4 columns)
2. **SQL file paths** (different per association)
3. **Return types** (different InsertedXxxModel types)
4. **Error message strings** (mention specific column names)

Because `sqlx::query_file_as!` requires a string literal for the SQL file path (not a variable or expression), a declarative macro cannot parameterize over SQL file paths. The macro would need to accept the file path as a token and pass it through, which works but produces a macro invocation nearly as long as the original function.

**Recommendation:** These remain manual for now. The `validate_insert_args!` macro from section 4.4 already handles the most common sub-pattern (column count validation in insert functions).

### 6c. Query Wrapper Pairs (Pattern 5)

The public->internal->fold SELECT chains (e.g., `select_tasks_by_universal_ids` -> `select_task_models_by_universal_ids` -> `FlatTaskModel::fold_models`) vary significantly in:

1. **Query parameters** (object_ids, slugs, user_id + filter_ops, etc.)
2. **SQL file paths** (string literals required by `query_file_as!`)
3. **Fold function** (some use `FlatModel::fold_models()`, some use standalone `fold_xxx()`)
4. **Return type wrapping** (some map `Into::into`, some call `.into()` on the model directly)

The `sqlx::query_file_as!` constraint (string literal SQL paths) means a declarative macro cannot generalize over the SQL file path. The variation in query parameters (Vec<Uuid> vs single Uuid vs two Vecs) makes the call signature too polymorphic for a single macro.

**Recommendation:** These remain manual. The boilerplate is real (~15-20 lines per function pair) but the variation is too high for a clean macro. A future proc macro could handle this.

---

## 7. Implementation Guidance

### 7.1 Migration Order

Implement macros in this order to minimize risk and maximize early value:

**Phase 1: Create macros.rs with all macro definitions (1 file)**

Create `crates/dapple-data/src/macros.rs` with all 7 macro definitions from sections 4 and 5 above. Add `#[macro_use] mod macros;` to `lib.rs`.

**Phase 2: Migrate Tier 1 macros across model files**

Start with the simplest entities that have the least variation:

1. `stages.rs` -- Simplest unified-versioned entity (no children, no extra fields beyond standard)
2. `categories.rs` -- Same structure as stages
3. `hats.rs` -- Stages + categories association
4. `tasks.rs` -- Standard + has children (task_prompts)
5. `steps.rs` -- Standard + has children (step_tasks)
6. Continue through remaining model files

For each file:
1. Replace `Keyed` + `RelationalObject` impls with `impl_keyed!`
2. Replace `InsertedModel` + `Keyed` + `RelationalObject` impls with `impl_keyed!(Xxx, inserted)`
3. Replace `VersionedRelationalObject` + `UnifiedVersionedObject` + `Depreciable` with `impl_versioned!`
4. Replace `InsertColumnArgs` impl with `impl_insert_column_args!`
5. Replace `DataTransferObject` impls with `impl_dto_object_id!`
6. Replace `FlatRelationalObject` impl with `impl_flat_to_parent!`
7. Or use the compound `impl_principal_traits!` where applicable

**Phase 3: Migrate Tier 1 macros across query files**

For each query file with a `validate_insert_xxx_args` function:
1. Replace the function with `validate_insert_args!`

**Phase 4: Verify**

After each file migration:
1. Run `cargo build -p dapple-data` to verify compilation
2. Run `cargo test -p dapple-data` (if unit tests exist)
3. Verify `cargo clippy -p dapple-data -- -D warnings` passes

### 7.2 Per-File Migration Template

For a typical unified-versioned entity like `stages.rs`, the trait implementation section transforms from:

```rust
// ============================================================================
// Trait Implementations (55 lines)
// ============================================================================

impl crate::traits::InsertedModel for InsertedStageModel {}

impl crate::Keyed for InsertedStageModel {
    fn id(&self) -> &i64 { &self.id }
    fn object_id(&self) -> &sqlx::types::Uuid { &self.object_id }
}

impl crate::traits::RelationalObject for InsertedStageModel {
    fn id(&self) -> &i64 { &self.id }
    fn object_id(&self) -> &sqlx::types::Uuid { &self.object_id }
}

impl crate::traits::RelationalObject for StageModel {
    fn id(&self) -> &i64 { <Self as crate::Keyed>::id(self) }
    fn object_id(&self) -> &sqlx::types::Uuid { <Self as crate::Keyed>::object_id(self) }
}

impl crate::traits::PrincipalObject for StageModel {
    fn display_name(&self) -> Option<&str> { Some(&self.name) }
}

impl crate::traits::VersionedRelationalObject for StageModel {
    fn version_major(&self) -> i16 { self.version_major as i16 }
    fn version_minor(&self) -> i16 { self.version_minor as i16 }
    fn version_patch(&self) -> i16 { self.version_patch as i16 }
}

impl crate::traits::UnifiedVersionedObject for StageModel {
    fn versioned_object_id(&self) -> i64 { self.versioned_object_id }
    fn path_object_id(&self) -> &Uuid { &self.path_object_id }
    fn entity_type_key(&self) -> &str {
        crate::model::versioned_objects::entity_type_keys::STAGE
    }
}

impl crate::traits::Depreciable for StageModel {
    fn deprecation_level(&self) -> Option<&str> {
        self.deprecation_level_key.as_deref()
    }
}

impl crate::traits::FlatRelationalObject for FlatStageModel {
    type ParentInserted = InsertedStageModel;
    fn to_parent_inserted(&self) -> Self::ParentInserted {
        InsertedStageModel {
            id: self.id,
            object_id: self.object_id,
            versioned_object_id: self.versioned_object_id,
            path_id: self.path_id,
            path_object_id: self.path_object_id,
            name: self.name.clone(),
            slug: self.slug.clone(),
            version_major: self.version_major,
            version_minor: self.version_minor,
            version_patch: self.version_patch,
            description: self.description.clone(),
            created_at: self.created_at,
        }
    }
}

impl crate::traits::DataTransferObject for Stage {
    fn object_id(&self) -> &sqlx::types::Uuid { &self.object_id }
}
```

Into:

```rust
// ============================================================================
// Trait Implementations (14 lines)
// ============================================================================

impl_principal_traits! {
    entity_key: STAGE,
    inserted: InsertedStageModel,
    model: StageModel,
    dto: Stage,
    display_field: name,
}

impl_flat_to_parent! {
    FlatStageModel => InsertedStageModel {
        copy: [id, object_id, versioned_object_id, path_id, path_object_id,
               version_major, version_minor, version_patch, created_at],
        clone: [name, slug, description],
    }
}
```

And the `InsertColumnArgs` section transforms from:

```rust
impl InsertColumnArgs for InsertStageArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("names", self.names.len()),
            ("descriptions", self.descriptions.len()),
        ])
    }
}
```

Into:

```rust
impl_insert_column_args!(InsertStageArgs, [names, descriptions]);
```

### 7.3 Non-Versioned Entities

Entities like `JourneyModel`, `DispatchModel`, and `ApiTokenModel` are **not** unified-versioned. They lack `versioned_object_id`, `path_object_id`, `version_major/minor/patch`, and `deprecation_level_key` fields.

For these entities, use the lower-level macros individually:

```rust
// journeys.rs
impl_keyed!(InsertedJourneyModel, inserted);
impl_keyed!(JourneyModel);
impl_dto_object_id!(Journey);

// Manual: RelationalObject for JourneyModel (delegates to Keyed)
// Manual: PrincipalObject for JourneyModel
// Manual: FlatRelationalObject for FlatJourneyModel
```

Do NOT use `impl_principal_traits!` or `impl_versioned!` on non-versioned entities.

### 7.4 Child/Dependent Models

Child models like `StepTaskModel`, `PlaybookStageModel`, `PlaybookStepModel` can still use:

```rust
impl_keyed!(InsertedStepTaskModel, inserted);
impl_keyed!(StepTaskModel);
impl_dto_object_id!(StepTask);
```

Their `OwnedDependent` impls remain manual because the parent field names vary (`step_id`, `playbook_id`, `playbook_stage_id`).

---

## 8. Complete macros.rs File

```rust
//! Declarative macros for reducing boilerplate in dapple-data model and query files.
//!
//! These macros generate identical trait implementations that are repeated across
//! dozens of entity types. All macros expand to the exact same code that was
//! previously written by hand.
//!
//! ## Macro Index
//!
//! ### Tier 1 -- Pure mechanical
//! - [`impl_keyed!`] -- `Keyed` + `RelationalObject` (+ optional `InsertedModel`)
//! - [`impl_versioned!`] -- `VersionedRelationalObject` + `UnifiedVersionedObject` + `Depreciable`
//! - [`impl_insert_column_args!`] -- `InsertColumnArgs` from field list
//! - [`validate_insert_args!`] -- Column-count validation function
//!
//! ### Tier 2 -- Compound/structural
//! - [`impl_principal_traits!`] -- Full trait bundle for unified-versioned principals
//! - [`impl_dto_object_id!`] -- `DataTransferObject` for DTOs with `object_id`
//! - [`impl_flat_to_parent!`] -- `FlatRelationalObject::to_parent_inserted`

// ---------------------------------------------------------------------------
// Tier 1
// ---------------------------------------------------------------------------

/// Implement `Keyed` and `traits::RelationalObject` for a type with `id: i64` and `object_id: Uuid`.
///
/// Requires the type to have fields: `pub id: i64` and `pub object_id: Uuid`.
///
/// # Variants
///
/// - `impl_keyed!(MyType)` -- implements Keyed + RelationalObject
/// - `impl_keyed!(MyType, inserted)` -- also implements InsertedModel marker trait
///
/// # Examples
///
/// ```ignore
/// impl_keyed!(InsertedTaskModel, inserted);
/// impl_keyed!(TaskModel);
/// ```
#[macro_export]
macro_rules! impl_keyed {
    ($ty:ty) => {
        impl $crate::Keyed for $ty {
            fn id(&self) -> &i64 {
                &self.id
            }
            fn object_id(&self) -> &sqlx::types::Uuid {
                &self.object_id
            }
        }

        impl $crate::traits::RelationalObject for $ty {
            fn id(&self) -> &i64 {
                &self.id
            }
            fn object_id(&self) -> &sqlx::types::Uuid {
                &self.object_id
            }
        }
    };

    ($ty:ty, inserted) => {
        impl_keyed!($ty);
        impl $crate::traits::InsertedModel for $ty {}
    };
}

/// Implement versioning traits for a unified-versioned model type.
///
/// Implements `VersionedRelationalObject`, `UnifiedVersionedObject`, and `Depreciable`.
///
/// Requires the type to have fields:
/// - `version_major: i32`, `version_minor: i32`, `version_patch: i32`
/// - `versioned_object_id: i64`
/// - `path_object_id: Uuid`
/// - `deprecation_level_key: Option<String>`
///
/// The second argument is the constant name from `entity_type_keys` (e.g., `TASK`, `PLAYBOOK`).
///
/// # Examples
///
/// ```ignore
/// impl_versioned!(TaskModel, TASK);
/// impl_versioned!(PlaybookModel, PLAYBOOK);
/// ```
#[macro_export]
macro_rules! impl_versioned {
    ($ty:ty, $entity_key:ident) => {
        impl $crate::traits::VersionedRelationalObject for $ty {
            fn version_major(&self) -> i16 {
                self.version_major as i16
            }
            fn version_minor(&self) -> i16 {
                self.version_minor as i16
            }
            fn version_patch(&self) -> i16 {
                self.version_patch as i16
            }
        }

        impl $crate::traits::UnifiedVersionedObject for $ty {
            fn versioned_object_id(&self) -> i64 {
                self.versioned_object_id
            }
            fn path_object_id(&self) -> &sqlx::types::Uuid {
                &self.path_object_id
            }
            fn entity_type_key(&self) -> &str {
                $crate::model::versioned_objects::entity_type_keys::$entity_key
            }
        }

        impl $crate::traits::Depreciable for $ty {
            fn deprecation_level(&self) -> Option<&str> {
                self.deprecation_level_key.as_deref()
            }
        }
    };
}

/// Implement `InsertColumnArgs` for a struct by listing its Vec fields.
///
/// Each field must be a `Vec<T>` that supports `.len()`.
///
/// # Examples
///
/// ```ignore
/// impl_insert_column_args!(InsertTaskArgs, [names, descriptions, functions, function_inputs]);
/// impl_insert_column_args!(InsertStageArgs, [names, descriptions]);
/// ```
#[macro_export]
macro_rules! impl_insert_column_args {
    ($ty:ty, [$($field:ident),+ $(,)?]) => {
        impl $crate::InsertColumnArgs for $ty {
            fn column_counts(&self) -> Result<Vec<(&str, usize)>, $crate::error::DataError> {
                Ok(vec![
                    $( (stringify!($field), self.$field.len()), )+
                ])
            }
        }
    };
}

/// Generate a validation function for insert args slices.
///
/// The generated function checks that all items in the slice have
/// consistent column counts using the `InsertColumnArgs` trait.
///
/// # Examples
///
/// ```ignore
/// validate_insert_args!(validate_insert_task_args, InsertTaskArgs);
/// // Generates: fn validate_insert_task_args(objs: &[InsertTaskArgs]) -> Result<(), DataError>
/// ```
#[macro_export]
macro_rules! validate_insert_args {
    ($fn_name:ident, $args_ty:ty) => {
        fn $fn_name(objs: &[$args_ty]) -> Result<(), $crate::error::DataError> {
            let mut expected = None;
            for obj in objs.iter() {
                if expected.is_none() {
                    expected = Some(obj.column_count()?);
                }
                obj.validate_column_counts(expected)?;
            }
            Ok(())
        }
    };
}

// ---------------------------------------------------------------------------
// Tier 2
// ---------------------------------------------------------------------------

/// Implement the full suite of traits for a unified-versioned principal object.
///
/// Bundles: `Keyed`, `RelationalObject`, `InsertedModel`, `PrincipalObject`,
/// `VersionedRelationalObject`, `UnifiedVersionedObject`, `Depreciable`,
/// and `DataTransferObject`.
///
/// # Arguments
///
/// - `entity_key` -- constant name from `entity_type_keys` (e.g., `TASK`)
/// - `inserted` -- the `InsertedXxxModel` type
/// - `model` -- the `XxxModel` type
/// - `dto` -- the public DTO type
/// - `display_field` -- (optional) field name for `PrincipalObject::display_name()`
///
/// # Examples
///
/// ```ignore
/// impl_principal_traits! {
///     entity_key: TASK,
///     inserted: InsertedTaskModel,
///     model: TaskModel,
///     dto: Task,
///     display_field: name,
/// }
/// ```
#[macro_export]
macro_rules! impl_principal_traits {
    (
        entity_key: $entity_key:ident,
        inserted: $inserted:ty,
        model: $model:ty,
        dto: $dto:ty,
        display_field: $display:ident $(,)?
    ) => {
        impl_keyed!($inserted, inserted);
        impl_keyed!($model);

        impl $crate::traits::PrincipalObject for $model {
            fn display_name(&self) -> Option<&str> {
                Some(&self.$display)
            }
        }

        impl_versioned!($model, $entity_key);

        impl $crate::traits::DataTransferObject for $dto {
            fn object_id(&self) -> &sqlx::types::Uuid {
                &self.object_id
            }
        }
    };

    (
        entity_key: $entity_key:ident,
        inserted: $inserted:ty,
        model: $model:ty,
        dto: $dto:ty $(,)?
    ) => {
        impl_keyed!($inserted, inserted);
        impl_keyed!($model);

        impl $crate::traits::PrincipalObject for $model {}

        impl_versioned!($model, $entity_key);

        impl $crate::traits::DataTransferObject for $dto {
            fn object_id(&self) -> &sqlx::types::Uuid {
                &self.object_id
            }
        }
    };
}

/// Implement `DataTransferObject` for a DTO type with `object_id: Uuid`.
///
/// # Examples
///
/// ```ignore
/// impl_dto_object_id!(StepTask);
/// impl_dto_object_id!(PlaybookStage);
/// ```
#[macro_export]
macro_rules! impl_dto_object_id {
    ($ty:ty) => {
        impl $crate::traits::DataTransferObject for $ty {
            fn object_id(&self) -> &sqlx::types::Uuid {
                &self.object_id
            }
        }
    };
}

/// Implement `FlatRelationalObject` for a flat model, providing `to_parent_inserted()`.
///
/// Fields in `copy` are assigned directly (for `Copy` types: i64, Uuid, i32, OffsetDateTime, etc.).
/// Fields in `clone` are `.clone()`d (for non-Copy types: String, Option<String>, JsonValue, Vec<u8>, etc.).
///
/// # Examples
///
/// ```ignore
/// impl_flat_to_parent! {
///     FlatTaskModel => InsertedTaskModel {
///         copy: [id, object_id, versioned_object_id, path_id, path_object_id,
///                version_major, version_minor, version_patch, created_at],
///         clone: [name, slug, description, function, function_input],
///     }
/// }
/// ```
#[macro_export]
macro_rules! impl_flat_to_parent {
    (
        $flat:ty => $inserted:ty {
            copy: [$($copy_field:ident),* $(,)?],
            clone: [$($clone_field:ident),* $(,)?] $(,)?
        }
    ) => {
        impl $crate::traits::FlatRelationalObject for $flat {
            type ParentInserted = $inserted;

            fn to_parent_inserted(&self) -> Self::ParentInserted {
                <$inserted> {
                    $( $copy_field: self.$copy_field, )*
                    $( $clone_field: self.$clone_field.clone(), )*
                }
            }
        }
    };
}
```

---

## 9. Testing Strategy

### Compilation Verification

The primary test is that `cargo build -p dapple-data` succeeds. Since the macros expand to the exact same code as the manual implementations, no new tests are needed. All existing tests continue to pass unchanged.

### Migration Verification Checklist

For each migrated file, verify:

1. `cargo build -p dapple-data` passes
2. `cargo clippy -p dapple-data -- -D warnings` passes
3. `cargo test --workspace` passes (if run against a database)
4. No new warnings introduced

### Macro Expansion Debugging

If a macro expansion fails, use `cargo expand` (from the `cargo-expand` tool) to see what the macro generates:

```bash
cargo expand -p dapple-data model::tasks 2>&1 | head -200
```

This shows the fully expanded Rust code and helps identify field name mismatches or type errors.

---

## 10. Entity Migration Reference

This table maps every unified-versioned entity to its macro invocations:

| Entity | `impl_principal_traits!` key | `impl_flat_to_parent!` clone fields | `impl_insert_column_args!` fields | Notes |
|---|---|---|---|---|
| Task | TASK | name, slug, description, function, function_input | names, descriptions, functions, function_inputs | Has TaskPromptRef child |
| Step | STEP | name, slug, description | names, descriptions | Has StepTask child |
| Stage | STAGE | name, slug, description | names, descriptions | Simplest entity |
| Category | CATEGORY | name, slug, description | names, descriptions | Same as Stage |
| Hat | HAT | name, slug, description | names, descriptions, categories | Has categories association |
| Playbook | PLAYBOOK | name, slug, description | names, descriptions, categories | Complex: stages, categories |
| Prompt | PROMPT | name, description | names, descriptions, fluent_messages | No slug |
| BriefGroup | BRIEF_GROUP | name, title, description | names, titles, descriptions | No slug |
| BriefPack | BRIEF_PACK | (check file) | (check file) | Has groups association |
| Brief | BRIEF | (check file) | (check file) | Brief units entity |
| Asset | ASSET | asset_kind_key, mime_type | (check file) | No name field |
| FluentMessage | FLUENT_MESSAGE | (check file) | (check file) | Complex body field |
| Directive | DIRECTIVE | (check file) | (check file) | Check file |
| ValueSlot | VALUE_SLOT | (check file) | (check file) | Check file |
| InferenceProfile | INFERENCE_PROFILE | name, description | names, descriptions | Has configs |
| Guide | GUIDE | (check file) | (check file) | Complex: chapters, blocks |

Entities that do NOT use `impl_principal_traits!` (non-versioned):
- JourneyModel, DispatchModel, ApiTokenModel, ServiceTokenModel, UserDetailsModel, etc.
- These use `impl_keyed!` and `impl_dto_object_id!` individually.

---

## 11. AGENTS.md Updates

Add the following section to `crates/dapple-data/AGENTS.md`:

```markdown
## Declarative Macros

The crate provides macros in `src/macros.rs` to reduce boilerplate for model trait implementations.

### Quick Reference

| Macro | Purpose | Where Used |
|---|---|---|
| `impl_keyed!(Type)` | `Keyed` + `RelationalObject` | Model files |
| `impl_keyed!(Type, inserted)` | Above + `InsertedModel` | Model files |
| `impl_versioned!(Type, KEY)` | Version + Unified + Depreciable | Model files |
| `impl_insert_column_args!(Type, [fields])` | `InsertColumnArgs` | Model files |
| `validate_insert_args!(fn_name, Type)` | Validation function | Query files |
| `impl_principal_traits! { ... }` | Full trait bundle | Model files |
| `impl_dto_object_id!(Type)` | `DataTransferObject` | Model files |
| `impl_flat_to_parent! { ... }` | `FlatRelationalObject` | Model files |

### When to Use `impl_principal_traits!`

Use for any unified-versioned principal entity that has:
- `InsertedXxxModel` with `id: i64`, `object_id: Uuid`
- `XxxModel` with `versioned_object_id`, `path_object_id`, `version_major/minor/patch`, `deprecation_level_key`
- `Xxx` DTO with `object_id: Uuid`

Do NOT use for:
- Non-versioned entities (JourneyModel, DispatchModel)
- Child/dependent models (StepTaskModel, PlaybookStageModel)
- DTOs that use dummy IDs (PromptPath, BriefGroupPath)

### When to Use Individual Macros

- `impl_keyed!` -- Any type with real `id: i64` and `object_id: Uuid` fields
- `impl_dto_object_id!` -- Any DTO type with `object_id: Uuid`
- `impl_flat_to_parent!` -- Any FlatXxxModel with a corresponding InsertedXxxModel
- `validate_insert_args!` -- Any query file with a validate function for InsertXxxArgs slices
```

---

## 12. Open Questions and Risks

### Resolved

1. **Declarative vs proc macro?** -- Declarative. All patterns can be expressed with `macro_rules!` token matching. Proc macros add compile-time complexity and a separate crate.

2. **Single file vs multiple?** -- Single `macros.rs`. The macros are closely related and total ~250 lines of definitions.

3. **`#[macro_export]` vs module-scoped?** -- Use `#[macro_export]` with `#[macro_use] mod macros;` in `lib.rs`. This makes macros available throughout the crate. The `$crate::` prefix in expansions ensures correct path resolution.

### Remaining Risks

1. **Macro hygiene with `sqlx::types::Uuid`** -- The macros reference `sqlx::types::Uuid` directly. If a model file doesn't have `sqlx` in scope, this could fail. Mitigation: `sqlx` is a direct dependency of `dapple-data` and is always available. The `$crate::` prefix handles crate-local paths; `sqlx::types::Uuid` is fine because `sqlx` is in `Cargo.toml`.

2. **IDE support** -- rust-analyzer handles `macro_rules!` well for go-to-definition and autocomplete. The macros are simple enough that IDE support should be seamless.

3. **Error messages** -- When a macro invocation has a typo (e.g., wrong field name), the error will point to the macro expansion, not the call site. Mitigation: Rust 1.73+ shows both the expansion location and the call site. The field names in macro invocations serve as documentation.

4. **Incremental migration** -- Files can be migrated one at a time. A file can use macros for some trait impls and keep others manual. There is no "all-or-nothing" constraint.
