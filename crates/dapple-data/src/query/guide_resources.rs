//! Query functions for guide resources and block citations.

use crate::error::DataError;
use crate::model::guide_resources::{
    InsertGuideBlockCitationArgs, InsertGuideResourceArgs, InsertedGuideBlockCitationModel,
    InsertedGuideResourceModel,
};
use crate::{DbPool, InsertColumnArgs};
use sqlx::types::Uuid;
use time::OffsetDateTime;

// ============================================================================
// INSERT functions
// ============================================================================

/// Batch insert guide resource references.
///
/// Parameters are bound in alphabetical order of column names:
/// asset_ids ($1), authors ($2), guide_ids ($3), keys ($4),
/// retrieved_ats ($5), titles ($6), urls ($7).
pub async fn insert_guide_resources(
    pool: &DbPool,
    args: InsertGuideResourceArgs,
) -> Result<Vec<InsertedGuideResourceModel>, DataError> {
    args.validate_column_counts(None)?;

    let rows = sqlx::query_file_as!(
        InsertedGuideResourceModel,
        "assets/.sql/guide_resources/insert_guide_resources.sql",
        &args.asset_ids as &[Option<Uuid>],
        &args.authors as &[Option<String>],
        &args.guide_ids,
        &args.keys,
        &args.retrieved_ats as &[Option<OffsetDateTime>],
        &args.titles as &[Option<String>],
        &args.urls as &[Option<String>],
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Batch insert guide resource references within an existing transaction.
pub async fn insert_guide_resources_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    args: InsertGuideResourceArgs,
) -> Result<Vec<InsertedGuideResourceModel>, DataError> {
    args.validate_column_counts(None)?;

    let rows = sqlx::query_file_as!(
        InsertedGuideResourceModel,
        "assets/.sql/guide_resources/insert_guide_resources.sql",
        &args.asset_ids as &[Option<Uuid>],
        &args.authors as &[Option<String>],
        &args.guide_ids,
        &args.keys,
        &args.retrieved_ats as &[Option<OffsetDateTime>],
        &args.titles as &[Option<String>],
        &args.urls as &[Option<String>],
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// Batch insert guide block citations.
///
/// Parameters are bound in alphabetical order of column names:
/// block_ids ($1), pages ($2), resource_ids ($3).
pub async fn insert_guide_block_citations(
    pool: &DbPool,
    args: InsertGuideBlockCitationArgs,
) -> Result<Vec<InsertedGuideBlockCitationModel>, DataError> {
    args.validate_column_counts(None)?;

    let rows = sqlx::query_file_as!(
        InsertedGuideBlockCitationModel,
        "assets/.sql/guide_resources/insert_guide_block_citations.sql",
        &args.block_ids,
        &args.pages as &[Option<String>],
        &args.resource_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Batch insert guide block citations within an existing transaction.
pub async fn insert_guide_block_citations_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    args: InsertGuideBlockCitationArgs,
) -> Result<Vec<InsertedGuideBlockCitationModel>, DataError> {
    args.validate_column_counts(None)?;

    let rows = sqlx::query_file_as!(
        InsertedGuideBlockCitationModel,
        "assets/.sql/guide_resources/insert_guide_block_citations.sql",
        &args.block_ids,
        &args.pages as &[Option<String>],
        &args.resource_ids,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}
