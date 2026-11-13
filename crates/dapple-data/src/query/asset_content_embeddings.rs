use crate::DbPool;
use crate::InsertColumnArgs;
use crate::error::DataError;
use crate::model::asset_content_embeddings::{
    AssetContentEmbeddingModel, AssetEmbeddingSearchResult, InsertAssetContentEmbeddingArgs,
};
use pgvector::Vector;
use sqlx::types::Uuid;

// ============================================================================
// INSERT
// ============================================================================

/// Insert asset content embeddings.
///
/// Uses `sqlx::query_as()` with `include_str!()` instead of `query_file_as!`
/// because SQLx compile-time checking does not handle the pgvector `vector(768)`
/// type. This is the documented exception for pgvector integration.
pub async fn insert_asset_content_embeddings(
    pool: &DbPool,
    args: InsertAssetContentEmbeddingArgs,
) -> Result<Vec<AssetContentEmbeddingModel>, DataError> {
    args.validate_column_counts(None)?;

    // Convert Vec<Vec<f32>> to Vec<pgvector::Vector> for binding
    let vectors: Vec<Vector> = args
        .embeddings
        .iter()
        .map(|v| Vector::from(v.clone()))
        .collect();

    let rows = sqlx::query_as::<_, AssetContentEmbeddingModel>(include_str!(
        "../../assets/.sql/asset_content_embeddings/insert_asset_content_embeddings.sql"
    ))
    .bind(&args.asset_ids)
    .bind(&args.extraction_ids)
    .bind(&args.chunk_indices)
    .bind(&args.chunk_texts)
    .bind(&vectors)
    .bind(&args.source_positions)
    .bind(&args.metadatas)
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

// ============================================================================
// DELETE
// ============================================================================

/// Delete all embeddings for the given extraction. Returns the number of rows
/// removed.
pub async fn delete_asset_content_embeddings_by_extraction(
    pool: &DbPool,
    extraction_id: Uuid,
) -> Result<u64, DataError> {
    let result = sqlx::query(include_str!(
        "../../assets/.sql/asset_content_embeddings/delete_asset_content_embeddings_by_extraction.sql"
    ))
    .bind(extraction_id)
    .execute(pool.get_pool())
    .await?;

    Ok(result.rows_affected())
}

// ============================================================================
// SEARCH
// ============================================================================

/// Search asset content embeddings via pgvector cosine similarity. When
/// `asset_ids` is empty the search runs across all assets.
///
/// Uses `sqlx::query_as()` with `include_str!()` because the query parameter
/// `$1` is `vector(768)`, which SQLx compile-time checking cannot handle.
pub async fn search_asset_content_embeddings_by_asset_ids(
    pool: &DbPool,
    query_embedding: &[f32],
    asset_ids: &[Uuid],
    limit: i32,
) -> Result<Vec<AssetEmbeddingSearchResult>, DataError> {
    let vector = Vector::from(query_embedding.to_vec());

    let rows = sqlx::query_as::<_, AssetEmbeddingSearchResult>(include_str!(
        "../../assets/.sql/asset_content_embeddings/search_asset_content_embeddings_by_asset_ids.sql"
    ))
    .bind(&vector)
    .bind(asset_ids)
    .bind(limit)
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}
