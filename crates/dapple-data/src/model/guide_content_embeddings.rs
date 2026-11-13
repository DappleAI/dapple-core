use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

// ============================================================================
// Insert Args (Column-wise)
// ============================================================================

/// Column-wise insert arguments for guide content embeddings.
/// All vectors MUST be the same length.
#[derive(Clone, Debug)]
pub struct InsertGuideContentEmbeddingArgs {
    pub guide_ids: Vec<Uuid>,
    pub chapter_ids: Vec<Uuid>,
    pub block_ids: Vec<Uuid>,
    pub directive_type_ids: Vec<i16>,
    pub chunk_texts: Vec<String>,
    pub embeddings: Vec<Vec<f32>>,
    pub metadata: Vec<serde_json::Value>,
    pub generation_ids: Vec<Uuid>,
}

impl_insert_column_args!(
    InsertGuideContentEmbeddingArgs,
    [
        guide_ids,
        chapter_ids,
        block_ids,
        directive_type_ids,
        chunk_texts,
        embeddings,
        metadata,
        generation_ids
    ]
);

/// Column-wise insert arguments for guide embedding tags.
#[derive(Clone, Debug)]
pub struct InsertGuideEmbeddingTagArgs {
    pub embedding_ids: Vec<Uuid>,
    pub tag_ids: Vec<i16>,
}

impl_insert_column_args!(InsertGuideEmbeddingTagArgs, [embedding_ids, tag_ids]);

/// Column-wise insert arguments for guide embedding locales.
#[derive(Clone, Debug)]
pub struct InsertGuideEmbeddingLocaleArgs {
    pub embedding_ids: Vec<Uuid>,
    pub locale_ids: Vec<i16>,
}

impl_insert_column_args!(InsertGuideEmbeddingLocaleArgs, [embedding_ids, locale_ids]);

// ============================================================================
// Inserted Models (INSERT...RETURNING)
// ============================================================================

/// Inserted embedding generation model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideEmbeddingGenerationModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub created_at: OffsetDateTime,
}

/// Inserted embedding model returned from INSERT...RETURNING.
/// Uses `sqlx::FromRow` because this type is used with `sqlx::query_as()`
/// (runtime binding) rather than `query_file_as!()` due to pgvector type handling.
#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct InsertedGuideContentEmbeddingModel {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub chapter_id: Uuid,
    pub block_id: Uuid,
    pub directive_type_id: i16,
    pub created_at: OffsetDateTime,
}

/// Inserted embedding tag model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideEmbeddingTagModel {
    pub id: Uuid,
    pub embedding_id: Uuid,
    pub tag_id: i16,
    pub created_at: OffsetDateTime,
}

/// Inserted embedding locale model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedGuideEmbeddingLocaleModel {
    pub id: Uuid,
    pub embedding_id: Uuid,
    pub locale_id: i16,
    pub created_at: OffsetDateTime,
}

// ============================================================================
// Query Result Models
// ============================================================================

/// Search result row from a similarity query.
/// Returned as a flat model from pgvector similarity queries.
/// Uses `sqlx::FromRow` because this type is used with `sqlx::query_as()`
/// (runtime binding) rather than `query_file_as!()` due to pgvector type handling.
#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct GuideEmbeddingSearchResult {
    pub id: Uuid,
    pub guide_id: Uuid,
    pub chapter_id: Uuid,
    pub block_id: Uuid,
    pub directive_type_key: String,
    pub chunk_text: String,
    pub metadata: serde_json::Value,
    pub similarity: f64,
}

/// Public DTO for search results (API response).
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct GuideSearchResult {
    pub block_id: Uuid,
    pub guide_id: Uuid,
    pub chapter_id: Uuid,
    pub directive_type: String,
    pub chunk_text: String,
    #[schemars(with = "crate::JsonObject")]
    pub metadata: serde_json::Value,
    pub similarity: f64,
}

impl From<GuideEmbeddingSearchResult> for GuideSearchResult {
    fn from(m: GuideEmbeddingSearchResult) -> Self {
        Self {
            block_id: m.block_id,
            guide_id: m.guide_id,
            chapter_id: m.chapter_id,
            directive_type: m.directive_type_key,
            chunk_text: m.chunk_text,
            metadata: m.metadata,
            similarity: m.similarity,
        }
    }
}
