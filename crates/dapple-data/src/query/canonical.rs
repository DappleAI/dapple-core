//! Query functions for canonical release and variant pins.

use crate::error::DataError;
use crate::model::canonical::{
    CanonicalReleasePin, CanonicalReleasePinModel, InsertCanonicalReleasePinArgs,
    InsertCanonicalVariantPinArgs, ResolveGuidePinArgs, ResolvedGuidePinModel,
};
use crate::{DbPool, FilterOptions, InsertColumnArgs};
use log::debug;
use sqlx::types::Uuid;
use time::OffsetDateTime;

/// Batch resolve (ref_key, version) pairs to versioned_object_id via guides table.
pub async fn resolve_guide_version_pins(
    pool: &DbPool,
    args: &ResolveGuidePinArgs,
) -> Result<Vec<ResolvedGuidePinModel>, DataError> {
    args.validate_column_counts(None)?;

    let rows = sqlx::query_file_as!(
        ResolvedGuidePinModel,
        "assets/.sql/canonical/resolve_guide_version_pins.sql",
        &args.ref_keys,
        &args.content_hashes,
    )
    .fetch_all(pool.get_pool())
    .await?;

    debug!("Resolved {} guide version pins", rows.len());
    Ok(rows)
}

/// Insert canonical release pins with supersession (append-only ledger).
pub async fn insert_canonical_release_pins(
    pool: &DbPool,
    args: &InsertCanonicalReleasePinArgs,
) -> Result<Vec<CanonicalReleasePinModel>, DataError> {
    args.validate_column_counts(None)?;

    let rows = sqlx::query_file_as!(
        CanonicalReleasePinModel,
        "assets/.sql/canonical/insert_canonical_release_pins.sql",
        &args.scopes,
        &args.environments,
        &args.object_types,
        &args.ref_keys,
        &args.content_hashes,
        &args.versioned_object_ids as &[Option<Uuid>],
        &args.resolved_ats as &[Option<OffsetDateTime>]
    )
    .fetch_all(pool.get_pool())
    .await?;

    debug!("Inserted {} canonical release pins", rows.len());
    Ok(rows)
}

/// Insert canonical variant pins with supersession (append-only ledger).
pub async fn insert_canonical_variant_pins(
    pool: &DbPool,
    args: &InsertCanonicalVariantPinArgs,
) -> Result<(), DataError> {
    args.validate_column_counts(None)?;

    sqlx::query_file!(
        "assets/.sql/canonical/insert_canonical_variant_pins.sql",
        &args.scopes,
        &args.environments,
        &args.buckets,
        &args.object_types,
        &args.ref_keys,
        &args.content_hashes,
        &args.versioned_object_ids as &[Option<Uuid>],
        &args.resolved_ats as &[Option<OffsetDateTime>]
    )
    .fetch_all(pool.get_pool())
    .await?;

    debug!("Inserted canonical variant pins");
    Ok(())
}

/// Select all current (non-superseded) canonical release pins with pagination.
pub async fn select_canonical_release_pins(
    pool: &DbPool,
    filter_opts: FilterOptions,
) -> Result<Vec<CanonicalReleasePin>, DataError> {
    let (limit, offset) = filter_opts.sql_values();

    let rows = sqlx::query_file_as!(
        CanonicalReleasePinModel,
        "assets/.sql/canonical/select_canonical_release_pins.sql",
        limit,
        offset
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

/// Select current (non-superseded) canonical release pins filtered by scope and environment.
pub async fn select_canonical_release_pins_by_environment(
    pool: &DbPool,
    scope: &str,
    environment: &str,
    filter_opts: FilterOptions,
) -> Result<Vec<CanonicalReleasePin>, DataError> {
    let (limit, offset) = filter_opts.sql_values();

    let rows = sqlx::query_file_as!(
        CanonicalReleasePinModel,
        "assets/.sql/canonical/select_canonical_release_pins_by_environment.sql",
        scope,
        environment,
        limit,
        offset
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

/// Re-resolve any unresolved guide canonical pins against the current guide tables.
/// Called after guide ingest to pick up newly ingested versions.
/// Returns the pins that were newly resolved (as new append-only rows).
pub async fn resolve_unresolved_guide_pins(
    pool: &DbPool,
) -> Result<Vec<CanonicalReleasePinModel>, DataError> {
    let rows = sqlx::query_file_as!(
        CanonicalReleasePinModel,
        "assets/.sql/canonical/resolve_unresolved_guide_pins.sql"
    )
    .fetch_all(pool.get_pool())
    .await?;

    if !rows.is_empty() {
        debug!(
            "Re-resolved {} previously unresolved canonical guide pins",
            rows.len()
        );
    }
    Ok(rows)
}

/// Select current (non-superseded) canonical release pins by object_ids.
pub async fn select_canonical_release_pins_by_object_ids(
    pool: &DbPool,
    ids: &[Uuid],
) -> Result<Vec<CanonicalReleasePin>, DataError> {
    let rows = sqlx::query_file_as!(
        CanonicalReleasePinModel,
        "assets/.sql/canonical/select_canonical_release_pins_by_object_ids.sql",
        ids
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}
