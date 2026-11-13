use crate::DbPool;
use crate::error::DataError;
use crate::model::integrity::{
    AssetIntegrityIssue, AssetIntegrityIssueModel, ChunkChecksumMismatch,
    ChunkChecksumMismatchModel, ChunkChecksumMismatchRow, ChunkMissingChecksum,
    ChunkMissingChecksumModel, ChunkMissingChecksumRow, DuplicateChecksumModel,
    InsertedChecksumModel,
};

// ============================================================================
// Integrity Check Queries
// ============================================================================

/// Find chunks that are missing checksum records
pub async fn find_chunks_missing_checksums(
    pool: &DbPool,
) -> Result<Vec<ChunkMissingChecksum>, DataError> {
    let rows = sqlx::query_file_as!(
        ChunkMissingChecksumRow,
        "assets/.sql/integrity/find_chunks_missing_checksums.sql",
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ChunkMissingChecksumModel::from(r).into())
        .collect())
}

/// Verify that stored checksums match actual chunk data
/// Returns chunks where computed SHA-256 doesn't match stored checksum
pub async fn verify_chunk_checksums(
    pool: &DbPool,
) -> Result<Vec<ChunkChecksumMismatch>, DataError> {
    let rows = sqlx::query_file_as!(
        ChunkChecksumMismatchRow,
        "assets/.sql/integrity/verify_chunk_checksums.sql",
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ChunkChecksumMismatchModel::from(r).into())
        .collect())
}

/// Find checksums that point to multiple chunks (dedup failure detection)
pub async fn find_duplicate_checksums(
    pool: &DbPool,
) -> Result<Vec<DuplicateChecksumModel>, DataError> {
    let models = sqlx::query_file_as!(
        DuplicateChecksumModel,
        "assets/.sql/integrity/find_duplicate_checksums.sql",
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}

/// Find assets that reference chunks with integrity issues
pub async fn find_assets_with_integrity_issues(
    pool: &DbPool,
) -> Result<Vec<AssetIntegrityIssue>, DataError> {
    let models = sqlx::query_file_as!(
        AssetIntegrityIssueModel,
        "assets/.sql/integrity/find_assets_with_integrity_issues.sql",
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models.into_iter().map(|m| m.into()).collect())
}

// ============================================================================
// Integrity Repair Queries
// ============================================================================

/// Insert checksums for chunks that are missing them
/// Used by integrity service to repair missing checksum records
pub async fn insert_missing_checksums(
    pool: &DbPool,
) -> Result<Vec<InsertedChecksumModel>, DataError> {
    let models = sqlx::query_file_as!(
        InsertedChecksumModel,
        "assets/.sql/integrity/insert_missing_checksums.sql",
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models)
}
