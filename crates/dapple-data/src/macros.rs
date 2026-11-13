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
//! - [`impl_dto_id!`] -- `DataTransferObject` for DTOs with `id`
//! - [`impl_flat_to_parent!`] -- `FlatRelationalObject::to_parent_inserted`

// ---------------------------------------------------------------------------
// Tier 1
// ---------------------------------------------------------------------------

/// Implement `Keyed` and `traits::RelationalObject` for a type with `id: Uuid`.
///
/// Requires the type to have field: `pub id: Uuid`.
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
            fn id(&self) -> &sqlx::types::Uuid {
                &self.id
            }
        }

        impl $crate::traits::RelationalObject for $ty {
            fn id(&self) -> &sqlx::types::Uuid {
                &self.id
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
/// - `content_hash: String`
/// - `versioned_object_id: Uuid`
/// - `path_id: Uuid`
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
            fn content_hash(&self) -> &str {
                &self.content_hash
            }
        }

        impl $crate::traits::UnifiedVersionedObject for $ty {
            fn versioned_object_id(&self) -> &sqlx::types::Uuid {
                &self.versioned_object_id
            }
            fn path_id(&self) -> &sqlx::types::Uuid {
                &self.path_id
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
/// `VersionedRelationalObject`, `UnifiedVersionedObject`, and `Depreciable`.
///
/// Note: `DataTransferObject` is no longer generated here. DTO types that live
/// in `dapple-types` implement `DataTransferObject` in that crate to satisfy
/// Rust's orphan rules.
///
/// # Arguments
///
/// - `entity_key` -- constant name from `entity_type_keys` (e.g., `TASK`)
/// - `inserted` -- the `InsertedXxxModel` type
/// - `model` -- the `XxxModel` type
/// - `dto` -- the public DTO type (retained for compatibility but no longer used for trait impls)
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
    };
}

/// Implement `DataTransferObject` for a DTO type with `id: Uuid`.
///
/// Use this only for DTO types defined locally in dapple-data.
/// DTOs defined in dapple-types implement `DataTransferObject` in that crate
/// to satisfy Rust's orphan rules.
///
/// # Examples
///
/// ```ignore
/// impl_dto_id!(PromptResponse);
/// impl_dto_id!(Draft);
/// ```
#[macro_export]
macro_rules! impl_dto_id {
    ($ty:ty) => {
        impl $crate::traits::DataTransferObject for $ty {
            fn id(&self) -> &sqlx::types::Uuid {
                &self.id
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
///         copy: [id, versioned_object_id, path_id, created_at],
///         clone: [name, slug, description, function, function_input, content_hash],
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
                Self::ParentInserted {
                    $( $copy_field: self.$copy_field, )*
                    $( $clone_field: self.$clone_field.clone(), )*
                }
            }
        }
    };
}
