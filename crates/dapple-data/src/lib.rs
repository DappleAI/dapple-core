#[macro_use]
mod macros;

pub mod cache;
pub mod env;
pub mod error;
pub mod hash;
pub mod integrations;
pub mod model;
pub mod publisher;
pub mod query;
pub mod redis_cache;
pub mod redis_publisher;
pub mod reference;
pub mod slugify;
pub mod traits;
pub mod view;

pub use env::{load_env, load_env_with_fallback};
pub use slugify::slugify;

#[cfg(feature = "test-helpers")]
pub mod test_support;

use crate::error::InsertArgColumnMismatch::{EmptyColumnSet, InvalidColumnSize};
use crate::error::ResultSetProblems;
use crate::error::{DataError, InvalidIdErrors, ModelColumnMismatchError};
use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use std::fmt::Debug;
use std::time::Duration;

// Re-export types from dapple-types so downstream crates see no difference.
pub use dapple_types::{ContentHash, DateTime, FilterOptions, JsonObject};

/// Fixed UUID of the dapple system org in `rev.companies`. Stable across all
/// environments (sandbox, dev, stage, perf, prod) — seeded by Migration 31
/// and exposed to scripts via `$env.DAPPLE_COMPANY_UUID`. Internal handlers
/// pass this as `owner_company_id` when creating journeys/dispatches; every
/// journey/dispatch has an owning company, and dapple-team flows belong to
/// dapple.
pub const DAPPLE_COMPANY_UUID: Uuid = Uuid::from_u128(0x019e132e_2610_7a4a_b64a_77a49e286ac0);

// Pool defaults. The previous `sqlx::Pool::connect` convenience method gave us
// max_connections=10 and min_connections=0, which under bursty UI polling
// (3 parallel calls per cycle × multiple tabs × per-request service_token
// auth lookup) was both too small and too cold — every newly-spawned
// connection paid TCP+TLS+auth handshake on the request that caused it.
// 25 max leaves plenty of headroom against PG's max_connections=100 even
// with conduit/flow/ui sharing the cluster, and 5 min keeps connections
// warm for the steady-state poll load. Override via env if needed.
const DEFAULT_DB_MAX_CONNECTIONS: u32 = 25;
const DEFAULT_DB_MIN_CONNECTIONS: u32 = 5;
const DEFAULT_DB_ACQUIRE_TIMEOUT_SECS: u64 = 10;

// Cloning a `DbPool` clones the inner `sqlx::Pool` handle (a cheap Arc clone of
// the shared connection pool), not the connections themselves. Needed so the
// dispatch re-trigger can move an owned pool into spawned tasks.
#[derive(Clone)]
pub struct DbPool {
    pool: sqlx::Pool<sqlx::Postgres>,
}

impl DbPool {
    pub async fn connect() -> Result<Self, sqlx::Error> {
        let database_url = std::env::var("DAPPLE_PROMPT_DATABASE_URL")
            .expect("DAPPLE_PROMPT_DATABASE_URL must be set");

        let max_connections = env_u32("DAPPLE_DB_MAX_CONNECTIONS", DEFAULT_DB_MAX_CONNECTIONS);
        let min_connections = env_u32("DAPPLE_DB_MIN_CONNECTIONS", DEFAULT_DB_MIN_CONNECTIONS);
        let acquire_timeout_secs = env_u64(
            "DAPPLE_DB_ACQUIRE_TIMEOUT_SECS",
            DEFAULT_DB_ACQUIRE_TIMEOUT_SECS,
        );

        log::info!(
            "DbPool: max_connections={}, min_connections={}, acquire_timeout={}s",
            max_connections,
            min_connections,
            acquire_timeout_secs
        );

        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .min_connections(min_connections)
            .acquire_timeout(Duration::from_secs(acquire_timeout_secs))
            .connect(&database_url)
            .await?;

        Ok(DbPool { pool })
    }

    pub fn get_pool(&self) -> &sqlx::Pool<sqlx::Postgres> {
        &self.pool
    }

    /// Construct a `DbPool` from an existing `sqlx::PgPool`. Intended for
    /// integration tests that own their own pool (e.g. `#[sqlx::test]`),
    /// not for production use — the production path is `DbPool::connect()`,
    /// which honors the `DAPPLE_DB_*` tuning env vars.
    pub fn from_pool(pool: sqlx::Pool<sqlx::Postgres>) -> Self {
        DbPool { pool }
    }
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

pub trait DataModel {
    type DataType: Clone
        + Debug
        + Eq
        + PartialEq
        + JsonSchema
        + Serialize
        + DeserializeOwned
        + From<Self::DataType>;

    fn to_data_type(&self) -> Self::DataType;
}

#[macro_export]
macro_rules! columns_from_tuples {
    ($iter:expr) => {{
        let iter = $iter;
        let out = ::itertools::multiunzip(iter);
        out
    }};
}

pub trait Keyed {
    fn id(&self) -> &Uuid;
}

pub trait MergeOrdered {
    type Parent;
    type ChildRow;
    type Child: From<Self::ChildRow> + Keyed;
    type Final;
    type Error;

    fn parent_id(p: &Self::Parent) -> &Uuid;

    fn child_id(c: &Self::ChildRow) -> &Uuid;

    fn finalize(parent: &Self::Parent, children: Vec<Self::Child>) -> Self::Final;

    fn merge_ordered(
        parents: &[Self::Parent],
        children: impl IntoIterator<Item = Self::ChildRow>,
    ) -> Result<Vec<Self::Final>, Self::Error>
    where
        Self::Error: From<ModelColumnMismatchError>,
    {
        use ModelColumnMismatchError::ChildWithoutParent;

        // Group children by parent id, preserving arrival order within each
        // group (so ordinal-sensitive children keep their order). Grouping by
        // key makes the fold independent of the order children arrive in across
        // parents: a backing query that sorts children by their own id instead
        // of the parent FK can no longer silently mis-group or drop a parent's
        // children. The remaining integrity check is that every child maps to a
        // known parent.
        let mut groups: std::collections::HashMap<Uuid, Vec<Self::Child>> =
            std::collections::HashMap::new();
        for c in children {
            groups
                .entry(*Self::child_id(&c))
                .or_default()
                .push(c.into());
        }

        let mut out = Vec::with_capacity(parents.len());
        for p in parents.iter() {
            let kids = groups.remove(Self::parent_id(p)).unwrap_or_default();
            out.push(Self::finalize(p, kids));
        }

        if let Some((child_key, _)) = groups.into_iter().next() {
            return Err(ChildWithoutParent {
                child_key: child_key.to_string(),
            }
            .into());
        }

        Ok(out)
    }

    /// Just build and concatenate **children** in parent order (no parent
    /// finalize). Useful to cascade: build Steps from StepRows, then attach
    /// Tasks, etc. Order-independent grouping (see [`MergeOrdered::merge_ordered`]).
    fn merge_children_only(
        parents: &[Self::Parent],
        children: impl IntoIterator<Item = Self::ChildRow>,
    ) -> Result<Vec<Self::Child>, Self::Error>
    where
        Self::Error: From<ModelColumnMismatchError>,
    {
        use ModelColumnMismatchError::ChildWithoutParent;

        let mut groups: std::collections::HashMap<Uuid, Vec<Self::Child>> =
            std::collections::HashMap::new();
        for c in children {
            groups
                .entry(*Self::child_id(&c))
                .or_default()
                .push(c.into());
        }

        let mut out: Vec<Self::Child> = Vec::new();
        for p in parents.iter() {
            if let Some(mut kids) = groups.remove(Self::parent_id(p)) {
                out.append(&mut kids);
            }
        }

        if let Some((child_key, _)) = groups.into_iter().next() {
            return Err(ChildWithoutParent {
                child_key: child_key.to_string(),
            }
            .into());
        }

        Ok(out)
    }

    /// Attach **already-built children** (that implement `Keyed`) to parents.
    /// This lets you cascade after enriching children with their own children.
    /// Order-independent grouping (see [`MergeOrdered::merge_ordered`]).
    fn attach_ordered(
        parents: &[Self::Parent],
        children: impl IntoIterator<Item = Self::Child>,
    ) -> Result<Vec<Self::Final>, Self::Error>
    where
        Self::Error: From<ModelColumnMismatchError>,
    {
        use ModelColumnMismatchError::ChildWithoutParent;

        let mut groups: std::collections::HashMap<Uuid, Vec<Self::Child>> =
            std::collections::HashMap::new();
        for c in children {
            groups.entry(*c.id()).or_default().push(c);
        }

        let mut out = Vec::with_capacity(parents.len());
        for p in parents.iter() {
            let kids = groups.remove(Self::parent_id(p)).unwrap_or_default();
            out.push(Self::finalize(p, kids));
        }

        if let Some((child_key, _)) = groups.into_iter().next() {
            return Err(ChildWithoutParent {
                child_key: child_key.to_string(),
            }
            .into());
        }

        Ok(out)
    }

    /// build → attach in one pass (rows → children → parent finals)
    #[inline]
    fn merge_with_children<I>(
        parents: &[Self::Parent],
        children_rows: I,
    ) -> Result<Vec<Self::Final>, Self::Error>
    where
        I: IntoIterator<Item = Self::ChildRow>,
        Self::Error: From<ModelColumnMismatchError>,
    {
        Self::merge_ordered(parents, children_rows)
    }

    /// attach already-built, ordered children
    #[inline]
    fn attach_built_children<I>(
        parents: &[Self::Parent],
        children: I,
    ) -> Result<Vec<Self::Final>, Self::Error>
    where
        I: IntoIterator<Item = Self::Child>,
        Self::Error: From<ModelColumnMismatchError>,
    {
        Self::attach_ordered(parents, children)
    }

    /// build-only (emit children; no finalize-on-parent)
    #[inline]
    fn collect_children_for<I>(
        parents: &[Self::Parent],
        children_rows: I,
    ) -> Result<Vec<Self::Child>, Self::Error>
    where
        I: IntoIterator<Item = Self::ChildRow>,
        Self::Error: From<ModelColumnMismatchError>,
    {
        Self::merge_children_only(parents, children_rows)
    }
}

pub trait InsertColumnArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError>;

    fn column_count(&self) -> Result<usize, DataError> {
        match self.column_counts()?.first() {
            Some((_, count)) => Ok(*count),
            None => Err(EmptyColumnSet {}.into()),
        }
    }

    fn validate_column_counts(&self, expected: Option<usize>) -> Result<(), DataError> {
        let counts = self.column_counts()?;

        let expected = match expected {
            Some(e) => e,
            None => self.column_count()?,
        };

        for (name, found) in counts.iter().skip(1) {
            if *found != expected {
                return Err(InvalidColumnSize {
                    column: name.to_string(),
                    expected,
                    found: *found,
                }
                .into());
            }
        }
        Ok(())
    }

    fn validate_key_column_count<V>(&self, name: &str, key: Vec<V>) -> Result<(), DataError> {
        if key.len() != self.column_count()? {
            return Err(InvalidColumnSize {
                column: name.to_string(),
                expected: self.column_count()?,
                found: key.len(),
            }
            .into());
        }
        Ok(())
    }
}

fn get_single_row_by_id<M: Clone + Keyed>(id: Uuid, objs: Vec<M>) -> Result<M, DataError> {
    let mut unexpected_ids = Vec::new();
    let mut duplicated_ids = Vec::new();

    let mut found = false;

    if objs.is_empty() {
        return Err(EmptyColumnSet {}.into());
    }

    for obj in &objs {
        if obj.id() == &id {
            if found {
                duplicated_ids.push(id.to_string());
            } else {
                found = true;
            }
        } else {
            unexpected_ids.push(obj.id().to_string());
        }
    }

    if !unexpected_ids.is_empty() || !duplicated_ids.is_empty() || !found {
        let mut missing_ids = Vec::new();
        if !found {
            missing_ids.push(id.to_string());
        }

        return Err(ResultSetProblems::InvalidIds(InvalidIdErrors {
            missing_ids,
            duplicated_ids,
            unexpected_ids,
        })
        .into());
    }

    Ok(objs.first().unwrap().clone())
}
