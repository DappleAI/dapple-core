//! Embedding worker — runs semantic chunking over the text of a finished
//! `rev.asset_extractions` row and calls the conduit `/embed` endpoint to
//! produce vectors, then writes them to `rev.asset_content_embeddings`.
//!
//! Idempotent: before inserting we delete any existing embeddings for the
//! extraction so re-running a job replaces stale vectors.

use dapple_data::DbPool;
use dapple_data::model::asset_content_embeddings::InsertAssetContentEmbeddingArgs;
use dapple_data::model::asset_extraction_positions::AssetExtractionPositionModel;
use dapple_data::model::background_jobs::BackgroundJobModel;
use dapple_data::query::asset_content_embeddings::{
    delete_asset_content_embeddings_by_extraction, insert_asset_content_embeddings,
};
use dapple_data::query::asset_extraction_chunks::select_asset_extraction_chunks_by_extraction;
use dapple_data::query::asset_extraction_positions::select_asset_extraction_positions_by_extraction;
use dapple_data::query::asset_extractions::select_asset_extraction_by_id;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

/// Approximate tokens (characters) per embedded chunk — ModernBERT is good
/// up to ~500 tokens, and ~4 chars/token is a reasonable heuristic.
const SEMANTIC_CHUNK_SIZE: usize = 2_000;
/// Overlap between adjacent semantic chunks (chars).
const SEMANTIC_CHUNK_OVERLAP: usize = 400;
/// Conduit `/embed` call timeout — ONNX inference can be slow on cold
/// start.
const EMBED_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
    #[allow(dead_code)]
    dimension: usize,
}

/// Job handler for the `embed_extraction` background job. `target_id` is
/// the extraction id.
pub async fn handle_embed_extraction_job(
    pool: &DbPool,
    job: &BackgroundJobModel,
    conduit_url: Option<&str>,
) -> Result<(), String> {
    let extraction_id = job.target_id;

    // --- Load the extraction row ---------------------------------------
    let extraction = select_asset_extraction_by_id(pool, extraction_id)
        .await
        .map_err(|e| format!("select_asset_extraction_by_id: {e}"))?
        .ok_or_else(|| format!("extraction {extraction_id} not found"))?;

    if extraction.status != "ok" {
        return Err(format!(
            "extraction {extraction_id} has status {}, expected ok",
            extraction.status
        ));
    }
    let asset_id = extraction.asset_id;

    // --- Graceful degradation when conduit is unconfigured -------------
    let conduit_url = match conduit_url {
        Some(url) if !url.is_empty() => url.trim_end_matches('/').to_string(),
        _ => {
            log::warn!(
                "embed_extraction: DAPPLE_PROMPT_CONDUIT_URL not configured; skipping {}",
                extraction_id
            );
            return Ok(());
        }
    };

    // --- Reassemble the full text --------------------------------------
    let chunk_rows = select_asset_extraction_chunks_by_extraction(pool, extraction_id)
        .await
        .map_err(|e| format!("select_asset_extraction_chunks_by_extraction: {e}"))?;

    if chunk_rows.is_empty() {
        log::info!(
            "embed_extraction: extraction {} has no chunks, nothing to embed",
            extraction_id
        );
        return Ok(());
    }

    let mut full_bytes: Vec<u8> = Vec::new();
    for row in &chunk_rows {
        full_bytes.extend_from_slice(&row.data);
    }
    let full_text = String::from_utf8(full_bytes)
        .map_err(|e| format!("extraction {extraction_id} text is not utf-8: {e}"))?;

    if full_text.is_empty() {
        log::info!(
            "embed_extraction: extraction {} has empty text",
            extraction_id
        );
        return Ok(());
    }

    // --- Load positions for page mapping --------------------------------
    let positions = select_asset_extraction_positions_by_extraction(pool, extraction_id)
        .await
        .map_err(|e| format!("select_asset_extraction_positions_by_extraction: {e}"))?;

    // --- Semantic chunking ---------------------------------------------
    let semantic_chunks = semantic_chunk(&full_text);
    if semantic_chunks.is_empty() {
        log::info!(
            "embed_extraction: extraction {} produced zero semantic chunks",
            extraction_id
        );
        return Ok(());
    }

    // --- Call conduit /embed -------------------------------------------
    let texts: Vec<String> = semantic_chunks
        .iter()
        .map(|(_, text)| text.clone())
        .collect();

    let client = reqwest::Client::builder()
        .timeout(EMBED_TIMEOUT)
        .build()
        .map_err(|e| format!("build reqwest client: {e}"))?;

    let url = format!("{conduit_url}/embed");
    let response = client
        .post(&url)
        .json(&json!({ "texts": texts }))
        .send()
        .await
        .map_err(|e| format!("POST {url}: {e}"))?;

    let status = response.status();
    if status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
        log::warn!(
            "embed_extraction: conduit {url} returned 503; skipping {}",
            extraction_id
        );
        return Ok(());
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("POST {url} failed: {status} - {body}"));
    }

    let embed_response: EmbedResponse = response
        .json()
        .await
        .map_err(|e| format!("decode embed response: {e}"))?;

    if embed_response.embeddings.len() != semantic_chunks.len() {
        return Err(format!(
            "conduit returned {} embeddings, expected {}",
            embed_response.embeddings.len(),
            semantic_chunks.len()
        ));
    }

    // --- Idempotent replace --------------------------------------------
    delete_asset_content_embeddings_by_extraction(pool, extraction_id)
        .await
        .map_err(|e| format!("delete_asset_content_embeddings_by_extraction: {e}"))?;

    // --- Build insert args ---------------------------------------------
    let n = semantic_chunks.len();
    let mut asset_ids = Vec::with_capacity(n);
    let mut extraction_ids = Vec::with_capacity(n);
    let mut chunk_indices = Vec::with_capacity(n);
    let mut chunk_texts = Vec::with_capacity(n);
    let mut source_positions = Vec::with_capacity(n);
    let mut metadatas = Vec::with_capacity(n);

    for (idx, (offset, text)) in semantic_chunks.iter().enumerate() {
        let page = page_for_offset(&positions, *offset as i64);
        asset_ids.push(asset_id);
        extraction_ids.push(extraction_id);
        chunk_indices.push(idx as i32);
        chunk_texts.push(text.clone());
        source_positions.push(json!({
            "page": page,
            "offset": offset,
        }));
        metadatas.push(json!({}));
    }

    let embeddings = embed_response.embeddings;

    let args = InsertAssetContentEmbeddingArgs {
        asset_ids,
        extraction_ids,
        chunk_indices,
        chunk_texts,
        embeddings,
        source_positions,
        metadatas,
    };

    insert_asset_content_embeddings(pool, args)
        .await
        .map_err(|e| format!("insert_asset_content_embeddings: {e}"))?;

    log::info!(
        "embed_extraction: extraction {} -> {} embeddings written",
        extraction_id,
        n
    );

    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

/// Split `text` into fixed windows of `SEMANTIC_CHUNK_SIZE` with
/// `SEMANTIC_CHUNK_OVERLAP` between them. Returns `(byte_offset, chunk_text)`
/// pairs.
///
/// This is char-based (not token-based) — MVP approximation of ~500 tokens
/// / 100 overlap for ModernBERT-scale models.
fn semantic_chunk(text: &str) -> Vec<(usize, String)> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let total = chars.len();

    if total <= SEMANTIC_CHUNK_SIZE {
        chunks.push((0usize, text.to_string()));
        return chunks;
    }

    let stride = SEMANTIC_CHUNK_SIZE
        .saturating_sub(SEMANTIC_CHUNK_OVERLAP)
        .max(1);

    let mut start = 0usize;
    while start < total {
        let end = (start + SEMANTIC_CHUNK_SIZE).min(total);
        let byte_start = chars[start].0;
        let byte_end = if end < total {
            chars[end].0
        } else {
            text.len()
        };
        let chunk_text = text[byte_start..byte_end].to_string();
        chunks.push((byte_start, chunk_text));

        if end >= total {
            break;
        }
        start += stride;
    }

    chunks
}

/// Map a byte offset into the joined text to a 1-based page number using
/// the `rev.asset_extraction_positions` rows. Returns 0 when no matching
/// range is found (shouldn't happen in practice).
fn page_for_offset(positions: &[AssetExtractionPositionModel], offset: i64) -> i32 {
    for pos in positions {
        let start = pos.byte_offset;
        let end = pos.byte_offset + pos.byte_length;
        if offset >= start && offset < end {
            return pos
                .source_position
                .get("page")
                .and_then(|v| v.as_i64())
                .map(|p| p as i32)
                .unwrap_or(0);
        }
    }
    // Fallback: return the last page when offset is past the end (e.g.
    // when the chunk straddles the final page boundary).
    positions
        .last()
        .and_then(|p| p.source_position.get("page"))
        .and_then(|v| v.as_i64())
        .map(|p| p as i32)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_chunk_small_text() {
        let chunks = semantic_chunk("hello world");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].0, 0);
        assert_eq!(chunks[0].1, "hello world");
    }

    #[test]
    fn semantic_chunk_empty() {
        assert!(semantic_chunk("").is_empty());
    }

    #[test]
    fn semantic_chunk_overlaps() {
        let text: String = std::iter::repeat_n('a', 5_000).collect();
        let chunks = semantic_chunk(&text);
        assert!(chunks.len() > 1);
        // Every chunk except possibly the last should be the full window
        for (_, c) in chunks.iter().take(chunks.len() - 1) {
            assert_eq!(c.chars().count(), SEMANTIC_CHUNK_SIZE);
        }
    }
}
