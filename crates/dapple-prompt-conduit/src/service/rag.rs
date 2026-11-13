use crate::error::ConduitError;
use crate::service::onnx_client::OnnxInferenceClient;
use crate::service::EmbeddingService;
use dapple_data::model::guide_content_embeddings::GuideEmbeddingSearchResult;
use dapple_data::query::guide_content_embeddings::search_guide_content_embeddings_by_guide_ids_excluding;
use dapple_data::DbPool;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

/// Char budget for the text used as the retrieval QUERY.
///
/// When a task sets no `query_hint`, the query falls back to the entire
/// prompt — and prompts on EDGAR-style tasks run to 135k+ chars. Embedding
/// one of those as a single sequence is what OOM-killed conduit-onnx on
/// 2026-08-02 (~34k tokens, 12Gi limit, dead in 3s), which in turn failed
/// every guide sync that happened to be mid-flight.
///
/// ~4 chars/token against the embedding model's 8,192-token window, so any
/// query that already fit is unaffected; only the pathological case — which
/// previously failed outright — changes. conduit-onnx clamps again on its
/// side (`MAX_SEQUENCE_TOKENS`); this cap keeps us from shipping tens of
/// kilobytes over the wire just to have them discarded.
const MAX_QUERY_CHARS: usize = 32_768;

/// How many windows of the SAME guide block may occupy retrieval slots.
///
/// A block longer than dapple-prompt-api's `EMBED_WINDOW_CHARS` is stored as
/// several overlapping embedding rows (see that crate's `service::text_window`).
/// Those rows are near neighbours of one another in embedding space, so any
/// query that hits such a block tends to pull several of its windows into the
/// top-k at once — and with `limit` defaulting to 10, one long block could fill
/// the frame with near-identical excerpts while the rest of the guide is
/// starved. 2 keeps a long block's best-matching passage plus its runner-up.
const MAX_WINDOWS_PER_BLOCK: usize = 2;

/// Multiplier applied to `limit` before the per-block cap runs, so the frame
/// still fills to `limit` after filtering.
const RETRIEVAL_OVERFETCH: i32 = 3;

/// Absolute ceiling on rows fetched from pgvector.
///
/// **Not optional.** `RagContext::limit` arrives off the wire with no clamp
/// anywhere, and is passed straight to SQL `LIMIT`. Multiplying it by
/// [`RETRIEVAL_OVERFETCH`] without a ceiling would turn a caller's `limit:
/// 100000` into a 300k-row scan.
const MAX_RETRIEVAL_ROWS: i32 = 100;

/// Truncate on a UTF-8 char boundary, never a byte index.
fn clamp_query_text(text: &str) -> &str {
    if text.len() <= MAX_QUERY_CHARS {
        return text;
    }
    let end = text
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|i| *i <= MAX_QUERY_CHARS)
        .last()
        .unwrap_or(0);
    &text[..end]
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// RAG context for enriching agent prompts with guide content.
/// Assembled by Flow from dispatch context, passed per-request.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RagContext {
    /// Guide IDs to search within. Derived from the playbook's source guide.
    /// Empty means no RAG retrieval.
    #[serde(default)]
    pub guide_ids: Vec<Uuid>,

    /// Audience tags to filter by (e.g., ["agent", "universal"]).
    /// Derived from the task's tags or stage/step params.
    #[serde(default)]
    pub audience_tags: Vec<String>,

    /// Locale for the current execution (e.g., "en", "pt-br").
    #[serde(default = "default_locale")]
    pub locale: String,

    /// Maximum number of chunks to retrieve.
    #[serde(default = "default_rag_limit")]
    pub limit: i32,

    /// Block IDs already retrieved in prior steps of this dispatch.
    /// Conduit excludes these from results to avoid redundant context.
    #[serde(default)]
    pub exclude_block_ids: Vec<Uuid>,

    /// Optional semantic query override. If not set, Conduit uses
    /// the rendered prompt text as the query embedding source.
    #[serde(default)]
    pub query_hint: Option<String>,
}

fn default_locale() -> String {
    "en".to_string()
}

fn default_rag_limit() -> i32 {
    10
}

/// Internal struct representing retrieved RAG chunks ready for injection.
#[derive(Debug, Clone)]
pub struct RagResult {
    /// Formatted text block ready to inject into the prompt.
    pub context_text: String,
    /// Block IDs that were retrieved (for Flow to accumulate).
    pub retrieved_block_ids: Vec<Uuid>,
    /// Number of chunks retrieved.
    pub chunk_count: usize,
}

// ---------------------------------------------------------------------------
// RagService
// ---------------------------------------------------------------------------

/// Service for retrieving guide content via pgvector similarity search
/// and formatting it for injection into LLM prompts.
pub struct RagService {
    pool: Arc<DbPool>,
    /// Local embedding service (used when conduit-onnx is not deployed)
    embedding_service: Option<Arc<EmbeddingService>>,
    /// Remote embedding via conduit-onnx (preferred when available)
    onnx_client: Option<Arc<OnnxInferenceClient>>,
}

impl RagService {
    /// Create with local embedding service (legacy/fallback mode)
    pub fn new(pool: Arc<DbPool>, embedding_service: Arc<EmbeddingService>) -> Self {
        Self {
            pool,
            embedding_service: Some(embedding_service),
            onnx_client: None,
        }
    }

    /// Create with remote onnx client (sub-service mode)
    pub fn with_onnx_client(pool: Arc<DbPool>, onnx_client: Arc<OnnxInferenceClient>) -> Self {
        Self {
            pool,
            embedding_service: None,
            onnx_client: Some(onnx_client),
        }
    }

    /// Generate a single embedding vector using whichever backend is available.
    async fn embed_one(&self, text: &str) -> Result<Vec<f32>, ConduitError> {
        if let Some(ref client) = self.onnx_client {
            return client.embed_one(text).await;
        }
        if let Some(ref service) = self.embedding_service {
            return service.embed_one(text).await;
        }
        Err(ConduitError::EmbeddingError(
            "No embedding backend available for RAG".to_string(),
        ))
    }

    /// Retrieve and format RAG context for prompt injection.
    ///
    /// Returns an empty `RagResult` (with `context_text = ""`) if `guide_ids`
    /// is empty or no relevant chunks are found -- this is not an error.
    pub async fn retrieve(
        &self,
        rag_context: &RagContext,
        prompt_text: &str,
    ) -> Result<RagResult, ConduitError> {
        if rag_context.guide_ids.is_empty() {
            return Ok(RagResult {
                context_text: String::new(),
                retrieved_block_ids: vec![],
                chunk_count: 0,
            });
        }

        // 1. Build query text: prefer query_hint, fall back to prompt text.
        //    The fallback is why this must be bounded — an unhinted task makes
        //    its ENTIRE prompt the query. See MAX_QUERY_CHARS.
        let full_query = rag_context.query_hint.as_deref().unwrap_or(prompt_text);
        let query_text = clamp_query_text(full_query);

        if query_text.len() < full_query.len() {
            log::warn!(
                "RAG query text truncated from {} to {} chars before embedding. \
                 Set `query_hint` on this task to retrieve against a focused \
                 query instead of the whole prompt.",
                full_query.len(),
                query_text.len(),
            );
        }

        // 2. Generate query embedding
        let query_embedding = self.embed_one(query_text).await?;

        // 3. Query pgvector with exclusion support.
        //    Over-fetch, because step 4 drops rows (per-block window cap, tag
        //    fan-out, exclusions) and the frame should still reach `limit`.
        let tag_keys: Vec<String> = rag_context.audience_tags.clone();
        let want = rag_context.limit.max(0) as usize;
        let fetch_limit = rag_context
            .limit
            .saturating_mul(RETRIEVAL_OVERFETCH)
            .clamp(1, MAX_RETRIEVAL_ROWS);

        let results = search_guide_content_embeddings_by_guide_ids_excluding(
            &self.pool,
            &query_embedding,
            &rag_context.guide_ids,
            &tag_keys,
            &rag_context.locale,
            fetch_limit,
            &rag_context.exclude_block_ids,
        )
        .await
        .map_err(ConduitError::Database)?;

        // 4. Exclusions, row dedup and the per-block window cap, in one pass.
        //    (SQL also filters exclusions, but this guards against races.)
        let exclude_set: HashSet<Uuid> = rag_context.exclude_block_ids.iter().copied().collect();
        let filtered = apply_retrieval_caps(results, &exclude_set, want);

        // 5. Format into context text.
        //    `retrieved_block_ids` is deduped: with windowing, several rows can
        //    share a block_id, and this list feeds `exclude_block_ids` for
        //    cross-step dedup — which is block-granular, so repeats are noise.
        //    First-seen order is similarity order; never build this from a
        //    HashSet, which would randomize it.
        let mut seen_blocks: HashSet<Uuid> = HashSet::new();
        let retrieved_block_ids: Vec<Uuid> = filtered
            .iter()
            .filter(|r| seen_blocks.insert(r.block_id))
            .map(|r| r.block_id)
            .collect();
        let chunk_count = filtered.len();
        let context_text = format_rag_context(&filtered);

        Ok(RagResult {
            context_text,
            retrieved_block_ids,
            chunk_count,
        })
    }
}

// ---------------------------------------------------------------------------
// Retrieval shaping
// ---------------------------------------------------------------------------

/// Apply exclusion, row dedup and the per-block window cap in ONE pass over
/// rows that arrive in similarity order (`ORDER BY e.embedding <=> $1`).
///
/// Because the input is already ranked, taking rows greedily means each block
/// keeps its BEST windows and the surviving frame stays in similarity order —
/// no re-sort needed, and no scoring decisions made here.
///
/// Pure and pool-free so the shaping policy is unit-testable.
fn apply_retrieval_caps(
    results: Vec<GuideEmbeddingSearchResult>,
    exclude: &HashSet<Uuid>,
    limit: usize,
) -> Vec<GuideEmbeddingSearchResult> {
    let mut seen_rows: HashSet<Uuid> = HashSet::new();
    let mut per_block: HashMap<Uuid, usize> = HashMap::new();
    let mut out = Vec::with_capacity(limit.min(results.len()));

    for result in results {
        // Checked BEFORE the push, not after, so `limit == 0` yields nothing
        // rather than one row. `RagContext::limit` arrives off the wire, so 0
        // is a reachable input, not a theoretical one.
        if out.len() >= limit {
            break;
        }
        if exclude.contains(&result.block_id) {
            continue;
        }
        // PRE-EXISTING fan-out, unrelated to windowing: the search queries JOIN
        // guide_embedding_tags and guide_embedding_locales with no DISTINCT, so
        // an embedding matching two of the requested tags comes back TWICE and
        // would otherwise burn two slots on identical text.
        if !seen_rows.insert(result.id) {
            continue;
        }
        let count = per_block.entry(result.block_id).or_insert(0);
        if *count >= MAX_WINDOWS_PER_BLOCK {
            continue;
        }
        *count += 1;
        out.push(result);
    }

    out
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// Format search results into a system-level context block.
///
/// The format is designed to be clear to LLMs without wasting tokens
/// on elaborate markup. Each chunk is separated by a blank line.
///
/// Headers are deliberately NEUTRAL (`## Excerpt i/N`): guide and chapter
/// titles are audience-voiced copy (SUMMARY.md link text, guide.toml title)
/// that a model would read literally — an ironic chapter title becomes a
/// distractor framing the excerpt. The guide's CONDUITS.md (`# Guide Frame`)
/// is the sole interpretive key for retrieved prose; source identity, when a
/// guide needs the model to distinguish excerpt kinds, belongs there in the
/// author's conduit-facing vocabulary.
pub fn format_rag_context(results: &[GuideEmbeddingSearchResult]) -> String {
    if results.is_empty() {
        return String::new();
    }

    let mut parts = Vec::with_capacity(results.len() + 1);
    parts.push(
        "# Reference Context\n\nThe following excerpts from the guide are relevant to this task:\n"
            .to_string(),
    );

    for (i, result) in results.iter().enumerate() {
        parts.push(format!(
            "## Excerpt {}/{}\n*Type: {} | Relevance: {:.0}%*\n\n{}",
            i + 1,
            results.len(),
            result.directive_type_key,
            result.similarity * 100.0,
            result.chunk_text,
        ));
    }

    parts.join("\n\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_search_result(
        block_id: Uuid,
        chunk_text: &str,
        similarity: f64,
        guide_title: &str,
        chapter_title: &str,
        directive_type_key: &str,
    ) -> GuideEmbeddingSearchResult {
        let mut metadata = serde_json::Map::new();
        metadata.insert("guide_title".to_string(), json!(guide_title));
        if !chapter_title.is_empty() {
            metadata.insert("chapter_title".to_string(), json!(chapter_title));
        }

        GuideEmbeddingSearchResult {
            id: Uuid::now_v7(),
            guide_id: Uuid::now_v7(),
            chapter_id: Uuid::now_v7(),
            block_id,
            directive_type_key: directive_type_key.to_string(),
            chunk_text: chunk_text.to_string(),
            metadata: serde_json::Value::Object(metadata),
            similarity,
        }
    }

    /// A row with an explicit embedding-row `id`, so the duplicate-row case
    /// (tag JOIN fan-out) can be built. `make_search_result` generates a fresh
    /// id internally and cannot express it.
    fn row(id: Uuid, block_id: Uuid, similarity: f64) -> GuideEmbeddingSearchResult {
        GuideEmbeddingSearchResult {
            id,
            ..make_search_result(block_id, "chunk", similarity, "Guide", "Chapter", "prose")
        }
    }

    fn fresh_row(block_id: Uuid, similarity: f64) -> GuideEmbeddingSearchResult {
        row(Uuid::now_v7(), block_id, similarity)
    }

    // -- retrieval shaping --------------------------------------------------

    #[test]
    fn retrieval_caps_windows_per_block() {
        let long_block = Uuid::now_v7();
        let other_block = Uuid::now_v7();
        // Four windows of one long block outrank the only other block.
        let results = vec![
            fresh_row(long_block, 0.95),
            fresh_row(long_block, 0.94),
            fresh_row(long_block, 0.93),
            fresh_row(long_block, 0.92),
            fresh_row(other_block, 0.80),
        ];

        let kept = apply_retrieval_caps(results, &HashSet::new(), 10);

        assert_eq!(kept.len(), MAX_WINDOWS_PER_BLOCK + 1);
        assert_eq!(
            kept.iter().filter(|r| r.block_id == long_block).count(),
            MAX_WINDOWS_PER_BLOCK,
            "one block must not crowd out the rest of the guide"
        );
        assert!(kept.iter().any(|r| r.block_id == other_block));
    }

    #[test]
    fn retrieval_caps_keep_each_blocks_best_windows_in_order() {
        let block = Uuid::now_v7();
        let results = vec![
            fresh_row(block, 0.99),
            fresh_row(block, 0.88),
            fresh_row(block, 0.77),
        ];

        let kept = apply_retrieval_caps(results, &HashSet::new(), 10);

        // Input is similarity-ordered, so greedy take == best windows kept.
        let sims: Vec<f64> = kept.iter().map(|r| r.similarity).collect();
        assert_eq!(sims, vec![0.99, 0.88]);
    }

    #[test]
    fn retrieval_caps_fill_the_frame_to_the_limit() {
        // 30 over-fetched rows across many blocks, limit 10.
        let results: Vec<_> = (0..30)
            .map(|i| fresh_row(Uuid::now_v7(), 1.0 - (i as f64) / 100.0))
            .collect();

        let kept = apply_retrieval_caps(results, &HashSet::new(), 10);

        assert_eq!(
            kept.len(),
            10,
            "over-fetching must not under-fill the frame"
        );
    }

    #[test]
    fn retrieval_caps_drop_duplicate_rows_from_the_tag_join_fanout() {
        let id = Uuid::now_v7();
        let block = Uuid::now_v7();
        let other = Uuid::now_v7();
        // The SAME embedding row, returned twice because it matched two tags.
        let results = vec![
            row(id, block, 0.9),
            row(id, block, 0.9),
            fresh_row(other, 0.5),
        ];

        let kept = apply_retrieval_caps(results, &HashSet::new(), 2);

        assert_eq!(kept.len(), 2);
        assert_eq!(
            kept[1].block_id, other,
            "the duplicate must not burn a slot"
        );
    }

    #[test]
    fn retrieval_caps_honour_the_exclude_set() {
        let excluded = Uuid::now_v7();
        let kept_block = Uuid::now_v7();
        let results = vec![fresh_row(excluded, 0.99), fresh_row(kept_block, 0.10)];

        let kept = apply_retrieval_caps(results, &HashSet::from([excluded]), 10);

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].block_id, kept_block);
    }

    #[test]
    fn retrieval_caps_on_an_empty_or_zero_limit_input() {
        assert!(apply_retrieval_caps(vec![], &HashSet::new(), 10).is_empty());
        assert!(
            apply_retrieval_caps(vec![fresh_row(Uuid::now_v7(), 0.9)], &HashSet::new(), 0)
                .is_empty()
        );
    }

    #[test]
    fn test_format_rag_context_empty() {
        let result = format_rag_context(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn clamp_query_text_passes_through_normal_queries() {
        // A query that already fits must be returned byte-identical — this
        // cap changes nothing for the queries that worked before.
        let q = "which BDC filings disclose non-accrual loans?";
        assert_eq!(clamp_query_text(q), q);

        let at_limit = "a".repeat(MAX_QUERY_CHARS);
        assert_eq!(clamp_query_text(&at_limit).len(), MAX_QUERY_CHARS);
    }

    #[test]
    fn clamp_query_text_bounds_a_whole_prompt() {
        // The regression: an unhinted EDGAR task made its entire 135k-char
        // prompt the retrieval query, which OOM-killed conduit-onnx.
        let whole_prompt = "x".repeat(135_591);
        let clamped = clamp_query_text(&whole_prompt);
        assert!(
            clamped.len() <= MAX_QUERY_CHARS,
            "query must be bounded, got {} chars",
            clamped.len()
        );
        assert!(clamped.len() > MAX_QUERY_CHARS / 2, "must not over-trim");
    }

    #[test]
    fn clamp_query_text_cuts_on_a_char_boundary() {
        // Multi-byte chars straddling the cap must not panic or produce
        // invalid UTF-8. "é" is 2 bytes, so the budget lands mid-char.
        let multibyte = "é".repeat(MAX_QUERY_CHARS);
        let clamped = clamp_query_text(&multibyte);
        assert!(clamped.len() <= MAX_QUERY_CHARS);
        assert!(multibyte.starts_with(clamped));
        // Round-tripping proves the slice is valid UTF-8, not a split char.
        assert_eq!(
            std::str::from_utf8(clamped.as_bytes()).unwrap().len(),
            clamped.len()
        );
    }

    #[test]
    fn test_format_rag_context_single_chunk() {
        let results = vec![make_search_result(
            Uuid::now_v7(),
            "This is relevant content about essay writing.",
            0.85,
            "Essay Helper",
            "Getting Started",
            "prose",
        )];

        let formatted = format_rag_context(&results);
        assert!(formatted.contains("# Reference Context"));
        assert!(formatted.contains("## Excerpt 1/1"));
        assert!(formatted.contains("Type: prose | Relevance: 85%"));
        assert!(formatted.contains("This is relevant content about essay writing."));
        // Audience-voiced titles must never reach the conduit — CONDUITS.md
        // is the sole interpretive key for excerpts.
        assert!(!formatted.contains("Essay Helper"));
        assert!(!formatted.contains("Getting Started"));
    }

    #[test]
    fn test_format_rag_context_multiple_chunks() {
        let results = vec![
            make_search_result(
                Uuid::now_v7(),
                "First chunk about brainstorming.",
                0.92,
                "Essay Helper",
                "Brainstorming",
                "prose",
            ),
            make_search_result(
                Uuid::now_v7(),
                "Second chunk about outlining.",
                0.78,
                "Essay Helper",
                "Outlining",
                "stage",
            ),
        ];

        let formatted = format_rag_context(&results);
        assert!(formatted.contains("## Excerpt 1/2"));
        assert!(formatted.contains("## Excerpt 2/2"));
        assert!(formatted.contains("Relevance: 92%"));
        assert!(formatted.contains("Relevance: 78%"));
        assert!(!formatted.contains("Essay Helper"));
    }

    #[test]
    fn test_format_rag_context_no_chapter_title() {
        let results = vec![make_search_result(
            Uuid::now_v7(),
            "Top-level guide content.",
            0.70,
            "My Guide",
            "",
            "block",
        )];

        let formatted = format_rag_context(&results);
        // Neutral header regardless of metadata shape; no title breadcrumb.
        assert!(formatted.contains("## Excerpt 1/1"));
        assert!(!formatted.contains("My Guide"));
        assert!(!formatted.contains(" > "));
    }

    #[test]
    fn test_rag_context_serde_defaults() {
        let ctx: RagContext = serde_json::from_str("{}").unwrap();
        assert!(ctx.guide_ids.is_empty());
        assert!(ctx.audience_tags.is_empty());
        assert_eq!(ctx.locale, "en");
        assert_eq!(ctx.limit, 10);
        assert!(ctx.exclude_block_ids.is_empty());
        assert!(ctx.query_hint.is_none());
    }

    #[test]
    fn test_rag_context_serde_full() {
        let id = Uuid::now_v7();
        let json = json!({
            "guide_ids": [id],
            "audience_tags": ["agent"],
            "locale": "pt-br",
            "limit": 5,
            "exclude_block_ids": [],
            "query_hint": "essay topics"
        });
        let ctx: RagContext = serde_json::from_value(json).unwrap();
        assert_eq!(ctx.guide_ids.len(), 1);
        assert_eq!(ctx.locale, "pt-br");
        assert_eq!(ctx.limit, 5);
        assert_eq!(ctx.query_hint, Some("essay topics".to_string()));
    }
}
