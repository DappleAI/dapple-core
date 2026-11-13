//! Public traits that codify dapple-data relational concepts.
//!
//! These aim to make it explicit what kind of thing each `model::*` struct
//! represents (RelationalObject, VersionedRelationalObject, PathRelationalObject,
//! OwnedDependent, SharedDependent, FlatRelationalObject, InsertedModel, etc.).

use crate::ContentHash;
use sqlx::types::Uuid;

/// A basic relational object that maps to a single DB row and exposes an `id`.
/// Implemented for structs that have `id: Uuid`.
pub trait RelationalObject {
    fn id(&self) -> &Uuid;
}

/// Marker trait for types returned by INSERT ... RETURNING (inserted row models).
pub trait InsertedModel: RelationalObject {}

/// Marker for top-level principal objects (not owned by another object).
pub trait PrincipalObject: RelationalObject {
    /// Optional human-visible label/accessor; default None.
    fn display_name(&self) -> Option<&str> {
        None
    }
}

/// Versioned relational objects expose a content hash for content-addressable versioning.
pub trait VersionedRelationalObject {
    fn content_hash(&self) -> &str;

    fn to_content_hash(&self) -> ContentHash {
        ContentHash::new(self.content_hash().to_string())
    }
}

/// Path relational objects are "path" rows that relate to versioned objects.
pub trait PathRelationalObject: RelationalObject {
    /// Return the path's id (UUID). Many path tables use `path_id` as the UUID.
    fn path_id(&self) -> &Uuid;
}

/// Owned dependents are rows that belong to a single parent and carry the parent's UUID.
pub trait OwnedDependent {
    fn parent_id(&self) -> &Uuid;
}

/// Shared dependents are lightweight references that are shared between principals and only expose `id`.
pub trait SharedDependent {
    fn id(&self) -> &Uuid;
}

/// Flat relational object: a flattened row returned by queries that includes a parent and potentially owned dependents.
/// Implementations should set `ParentInserted` to the corresponding inserted-row model.
pub trait FlatRelationalObject {
    type ParentInserted: InsertedModel;

    /// Convert the flat row into the parent inserted model. Implementations will usually construct
    /// an Inserted*Model by copying the relevant fields.
    fn to_parent_inserted(&self) -> Self::ParentInserted;
}

// DataTransferObject trait is defined in dapple-types and re-exported here
// so that $crate::traits::DataTransferObject paths in macros continue to resolve.
pub use dapple_types::DataTransferObject;

// ============================================================================
// Unified Versioned Object Traits
// ============================================================================

/// Trait for models that use the unified versioning infrastructure.
///
/// This trait is implemented by entity models that reference the centralized
/// `versioned_objects` table instead of having per-entity version columns.
/// It provides a consistent interface for accessing versioning metadata.
pub trait UnifiedVersionedObject: RelationalObject {
    /// Returns the UUID of the associated versioned_objects row.
    fn versioned_object_id(&self) -> &Uuid;

    /// Returns the path_id (UUID) from the object_paths table.
    fn path_id(&self) -> &Uuid;

    /// Returns the entity type key (e.g., "task", "step", "playbook").
    fn entity_type_key(&self) -> &str;
}

/// Trait for models that have path details.
///
/// Path details contain the immutable, non-versioned information about an entity
/// (e.g., name, slug for tasks/steps/stages, or asset_kind_id for assets).
pub trait HasPathDetails {
    /// The type of path details for this entity.
    type PathDetails;

    /// Returns a reference to the path details.
    fn path_details(&self) -> &Self::PathDetails;
}

/// Trait for models that support deprecation.
///
/// Entities implementing this trait can be deprecated, retired, or quarantined
/// through the centralized deprecation_events system.
pub trait Depreciable {
    /// Returns the current deprecation level key, if any.
    /// `None` means the object is active (not deprecated).
    fn deprecation_level(&self) -> Option<&str>;

    /// Returns true if the object is not deprecated (deprecation_level is None or "active").
    fn is_active(&self) -> bool {
        matches!(self.deprecation_level(), None | Some("active"))
    }

    /// Returns true if the object is deprecated (any non-active deprecation level).
    fn is_deprecated(&self) -> bool {
        !self.is_active()
    }

    /// Returns true if the object is retired (cannot be used in new references).
    fn is_retired(&self) -> bool {
        self.deprecation_level() == Some("retired")
    }

    /// Returns true if the object is quarantined (hidden from most queries).
    fn is_quarantined(&self) -> bool {
        self.deprecation_level() == Some("quarantined")
    }
}
