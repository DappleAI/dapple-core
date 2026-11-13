use crate::error::DataError;
use crate::model::guide_content_embeddings::*;
use crate::{DbPool, InsertColumnArgs};
use pgvector::Vector;
use sqlx::types::Uuid;

// ============================================================================
// INSERT functions
// ============================================================================

/// Insert a new embedding generation for a guide.
/// Returns the generation row with its UUID. All embeddings in a batch
/// reference this generation_id for append-only versioning.
pub async fn insert_guide_embedding_generation(
    pool: &DbPool,
    guide_id: Uuid,
) -> Result<InsertedGuideEmbeddingGenerationModel, DataError> {
    let row = sqlx::query_file_as!(
        InsertedGuideEmbeddingGenerationModel,
        "assets/.sql/guide_content_embeddings/insert_guide_embedding_generation.sql",
        guide_id,
    )
    .fetch_one(pool.get_pool())
    .await?;

    Ok(row)
}

/// Insert guide content embeddings.
///
/// Uses `sqlx::query_as()` with `include_str!()` instead of `query_file_as!`
/// because SQLx compile-time checking does not handle the pgvector `vector(768)`
/// type. This is the documented exception for pgvector integration.
pub async fn insert_guide_content_embeddings(
    pool: &DbPool,
    args: InsertGuideContentEmbeddingArgs,
) -> Result<Vec<InsertedGuideContentEmbeddingModel>, DataError> {
    args.validate_column_counts(None)?;

    // Convert Vec<Vec<f32>> to Vec<pgvector::Vector> for binding
    let vectors: Vec<Vector> = args
        .embeddings
        .iter()
        .map(|v| Vector::from(v.clone()))
        .collect();

    let rows = sqlx::query_as::<_, InsertedGuideContentEmbeddingModel>(include_str!(
        "../../assets/.sql/guide_content_embeddings/insert_guide_content_embeddings.sql"
    ))
    .bind(&args.guide_ids)
    .bind(&args.chapter_ids)
    .bind(&args.block_ids)
    .bind(&args.directive_type_ids)
    .bind(&args.chunk_texts)
    .bind(&vectors)
    .bind(&args.metadata)
    .bind(&args.generation_ids)
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Insert guide embedding tag associations.
pub async fn insert_guide_embedding_tags(
    pool: &DbPool,
    args: InsertGuideEmbeddingTagArgs,
) -> Result<Vec<InsertedGuideEmbeddingTagModel>, DataError> {
    args.validate_column_counts(None)?;

    let rows = sqlx::query_file_as!(
        InsertedGuideEmbeddingTagModel,
        "assets/.sql/guide_content_embeddings/insert_guide_embedding_tags.sql",
        &args.embedding_ids,
        &args.tag_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Insert guide embedding locale associations.
pub async fn insert_guide_embedding_locales(
    pool: &DbPool,
    args: InsertGuideEmbeddingLocaleArgs,
) -> Result<Vec<InsertedGuideEmbeddingLocaleModel>, DataError> {
    args.validate_column_counts(None)?;

    let rows = sqlx::query_file_as!(
        InsertedGuideEmbeddingLocaleModel,
        "assets/.sql/guide_content_embeddings/insert_guide_embedding_locales.sql",
        &args.embedding_ids,
        &args.locale_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

// ============================================================================
// COUNT functions
// ============================================================================

/// Count the number of embeddings for a guide (latest generation only).
pub async fn count_guide_embeddings(pool: &DbPool, guide_id: Uuid) -> Result<i64, DataError> {
    let row = sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM rev.guide_content_embeddings e
        WHERE e.guide_id = $1
        AND e.generation_id = (
            SELECT g.id FROM rev.guide_embedding_generations g
            WHERE g.guide_id = $1
            ORDER BY g.created_at DESC
            LIMIT 1
        )
        "#,
        guide_id
    )
    .fetch_one(pool.get_pool())
    .await?;

    Ok(row.unwrap_or(0))
}

// ============================================================================
// SEARCH functions
// ============================================================================

/// Search guide content embeddings with audience and locale filtering.
///
/// Uses `sqlx::query_as()` with `include_str!()` because the query parameter
/// `$1` is `vector(768)`, which SQLx compile-time checking cannot handle.
pub async fn search_guide_content_embeddings(
    pool: &DbPool,
    query_embedding: &[f32],
    tag_keys: &[String],
    locale_tag: &str,
    limit: i32,
) -> Result<Vec<GuideEmbeddingSearchResult>, DataError> {
    let vector = Vector::from(query_embedding.to_vec());

    let rows = sqlx::query_as::<_, GuideEmbeddingSearchResult>(include_str!(
        "../../assets/.sql/guide_content_embeddings/search_guide_content_embeddings.sql"
    ))
    .bind(&vector)
    .bind(tag_keys)
    .bind(locale_tag)
    .bind(limit)
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Search guide content embeddings scoped to specific guides.
///
/// Uses `sqlx::query_as()` with `include_str!()` because the query parameter
/// `$1` is `vector(768)`, which SQLx compile-time checking cannot handle.
pub async fn search_guide_content_embeddings_by_guide_ids(
    pool: &DbPool,
    query_embedding: &[f32],
    guide_ids: &[Uuid],
    tag_keys: &[String],
    locale_tag: &str,
    limit: i32,
) -> Result<Vec<GuideEmbeddingSearchResult>, DataError> {
    let vector = Vector::from(query_embedding.to_vec());

    let rows = sqlx::query_as::<_, GuideEmbeddingSearchResult>(include_str!(
        "../../assets/.sql/guide_content_embeddings/search_guide_content_embeddings_by_guide_ids.sql"
    ))
    .bind(&vector)
    .bind(guide_ids)
    .bind(tag_keys)
    .bind(locale_tag)
    .bind(limit)
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Search guide content embeddings scoped to specific guides,
/// excluding blocks already retrieved in prior steps.
///
/// This variant supports the RAG pipeline's deduplication mechanism:
/// Flow tracks which block_ids have been retrieved across dispatch steps
/// and passes them as `exclude_block_ids` to avoid redundant context.
///
/// Uses `sqlx::query_as()` with `include_str!()` because the query parameter
/// `$1` is `vector(768)`, which SQLx compile-time checking cannot handle.
pub async fn search_guide_content_embeddings_by_guide_ids_excluding(
    pool: &DbPool,
    query_embedding: &[f32],
    guide_ids: &[Uuid],
    tag_keys: &[String],
    locale_tag: &str,
    limit: i32,
    exclude_block_ids: &[Uuid],
) -> Result<Vec<GuideEmbeddingSearchResult>, DataError> {
    let vector = Vector::from(query_embedding.to_vec());

    let rows = sqlx::query_as::<_, GuideEmbeddingSearchResult>(include_str!(
        "../../assets/.sql/guide_content_embeddings/search_guide_content_embeddings_by_guide_ids_excluding.sql"
    ))
    .bind(&vector)
    .bind(guide_ids)
    .bind(tag_keys)
    .bind(locale_tag)
    .bind(limit)
    .bind(exclude_block_ids)
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}
