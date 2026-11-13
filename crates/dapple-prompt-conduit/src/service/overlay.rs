use dapple_data::model::value_slots::ValueSlotPath;
use dapple_data::query::playbook_overlays::select_visible_value_slot_paths_for_task;
use dapple_data::DbPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::error::ConduitError;
use crate::service::embedding::EmbeddingService;
use crate::service::onnx_client::OnnxInferenceClient;

/// Configuration for overlay service
#[derive(Debug, Clone)]
pub struct OverlayConfig {
    /// Enable semantic ranking of value slots
    pub semantic_ranking: bool,
    /// Maximum number of semantically ranked results to return
    pub semantic_top_k: usize,
    /// Minimum similarity threshold for semantic matches
    pub similarity_threshold: f32,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            semantic_ranking: false,
            semantic_top_k: 10,
            similarity_threshold: 0.3,
        }
    }
}

impl OverlayConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            semantic_ranking: std::env::var("OVERLAY_SEMANTIC_RANKING")
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(false),
            semantic_top_k: std::env::var("OVERLAY_SEMANTIC_TOP_K")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10),
            similarity_threshold: std::env::var("OVERLAY_SIMILARITY_THRESHOLD")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.3),
        }
    }
}

/// Value slot with optional similarity score
#[derive(Debug, Clone)]
pub struct RankedValueSlot {
    pub slot: ValueSlotPath,
    pub similarity: Option<f32>,
}

impl From<ValueSlotPath> for RankedValueSlot {
    fn from(slot: ValueSlotPath) -> Self {
        Self {
            slot,
            similarity: None,
        }
    }
}

/// Service for resolving overlay visibility
pub struct OverlayService {
    pool: Arc<DbPool>,
    config: OverlayConfig,
    embedding_service: Option<Arc<EmbeddingService>>,
    onnx_client: Option<Arc<OnnxInferenceClient>>,
}

impl OverlayService {
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self {
            pool,
            config: OverlayConfig::default(),
            embedding_service: None,
            onnx_client: None,
        }
    }

    /// Create overlay service with configuration
    pub fn with_config(pool: Arc<DbPool>, config: OverlayConfig) -> Self {
        Self {
            pool,
            config,
            embedding_service: None,
            onnx_client: None,
        }
    }

    /// Create overlay service with local embeddings enabled
    pub fn with_embeddings(
        pool: Arc<DbPool>,
        config: OverlayConfig,
        embedding_service: Arc<EmbeddingService>,
    ) -> Self {
        Self {
            pool,
            config,
            embedding_service: Some(embedding_service),
            onnx_client: None,
        }
    }

    /// Create overlay service with remote onnx client for embeddings
    pub fn with_onnx_client(
        pool: Arc<DbPool>,
        config: OverlayConfig,
        onnx_client: Arc<OnnxInferenceClient>,
    ) -> Self {
        Self {
            pool,
            config,
            embedding_service: None,
            onnx_client: Some(onnx_client),
        }
    }

    /// Generate a single embedding using whichever backend is available.
    async fn embed_one(&self, text: &str) -> Result<Vec<f32>, ConduitError> {
        if let Some(ref client) = self.onnx_client {
            return client.embed_one(text).await;
        }
        if let Some(ref service) = self.embedding_service {
            return service.embed_one(text).await;
        }
        Err(ConduitError::EmbeddingError(
            "No embedding backend available for overlay".to_string(),
        ))
    }

    /// Resolve visible value slot paths for a playbook task
    /// Uses cascade resolution: stage -> step -> task (later overrides earlier)
    pub async fn resolve_visible_slots(
        &self,
        playbook_task_id: Uuid,
    ) -> Result<Vec<ValueSlotPath>, ConduitError> {
        let slots = select_visible_value_slot_paths_for_task(&self.pool, playbook_task_id).await?;
        Ok(slots)
    }

    /// Resolve visible slots with optional semantic ranking based on context
    /// If semantic ranking is enabled and context is provided, slots are ranked
    /// by similarity to the context. Otherwise, returns slots in standard order.
    pub async fn resolve_visible_slots_with_context(
        &self,
        playbook_task_id: Uuid,
        context: Option<&str>,
    ) -> Result<Vec<RankedValueSlot>, ConduitError> {
        // Get base visible slots
        let slots = self.resolve_visible_slots(playbook_task_id).await?;

        // If no context or semantic ranking disabled, return unranked
        let has_embedding = self.embedding_service.is_some() || self.onnx_client.is_some();
        let context = match context {
            Some(ctx) if self.config.semantic_ranking && has_embedding => ctx,
            _ => {
                return Ok(slots.into_iter().map(RankedValueSlot::from).collect());
            }
        };

        // Generate embedding for context
        let context_embedding = match self.embed_one(context).await {
            Ok(emb) => emb,
            Err(e) => {
                log::warn!(
                    "Failed to generate context embedding, falling back to unranked: {}",
                    e
                );
                return Ok(slots.into_iter().map(RankedValueSlot::from).collect());
            }
        };

        // For now, we do a simple client-side ranking by generating embeddings
        // for each slot's title+description and computing similarity.
        // In production, this could use the database-side similarity search.
        let mut ranked_slots = Vec::with_capacity(slots.len());

        for slot in slots {
            // Create text from slot metadata for embedding comparison
            let slot_text = format!("{} {}", slot.name, slot.value_kind);

            let similarity = match self.embed_one(&slot_text).await {
                Ok(slot_embedding) => Some(cosine_similarity(&context_embedding, &slot_embedding)),
                Err(_) => None,
            };

            ranked_slots.push(RankedValueSlot { slot, similarity });
        }

        // Sort by similarity (highest first), then by name for stability
        ranked_slots.sort_by(|a, b| match (a.similarity, b.similarity) {
            (Some(sim_a), Some(sim_b)) => sim_b
                .partial_cmp(&sim_a)
                .unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.slot.name.cmp(&b.slot.name),
        });

        // Filter by threshold and limit
        let ranked_slots: Vec<_> = ranked_slots
            .into_iter()
            .filter(|s| {
                s.similarity
                    .map(|sim| sim >= self.config.similarity_threshold)
                    .unwrap_or(true)
            })
            .take(self.config.semantic_top_k)
            .collect();

        Ok(ranked_slots)
    }

    /// Check if semantic ranking is available
    pub fn has_semantic_ranking(&self) -> bool {
        self.config.semantic_ranking
            && (self.embedding_service.is_some() || self.onnx_client.is_some())
    }
}

/// Compute cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}
