//! PDF extractor and its background-job handler.
//!
//! The handler:
//! 1. Loads the asset bytes from `rev.chunks` via `read_asset_content`.
//! 2. Short-circuits if an ok extraction already exists for the asset at
//!    `EXTRACTOR_VERSION_PDF`.
//! 3. Inserts a new `rev.asset_extractions` row in `pending`, transitions
//!    to `running`, and runs `PdfExtractor::extract` on the bytes.
//! 4. Chunks the full_text bytes into 256 KiB raw chunks, inserts them
//!    via `insert_raw_chunks`, and links them through
//!    `rev.asset_extraction_chunks`.
//! 5. Writes per-page `rev.asset_extraction_positions` rows.
//! 6. Marks the extraction `ok` and enqueues the chained embedding job.

use async_trait::async_trait;
use dapple_data::DbPool;
use dapple_data::model::asset_extraction_chunks::InsertAssetExtractionChunkArgs;
use dapple_data::model::asset_extraction_positions::InsertAssetExtractionPositionArgs;
use dapple_data::model::asset_extractions::InsertAssetExtractionArgs;
use dapple_data::model::background_jobs::BackgroundJobModel;
use dapple_data::query::asset_extraction_chunks::insert_asset_extraction_chunks;
use dapple_data::query::asset_extraction_positions::insert_asset_extraction_positions;
use dapple_data::query::asset_extractions::{
    insert_asset_extractions, mark_asset_extraction_failed, mark_asset_extraction_ok,
    mark_asset_extraction_running, select_latest_asset_extraction,
};
use dapple_data::query::chunks::{insert_raw_chunks, read_asset_content};
use serde_json::json;
use uuid::Uuid;

use super::{
    EXTRACTOR_KIND_PDF, EXTRACTOR_VERSION_PDF, Extractor, ExtractorError, ExtractorOutput,
    PageSegment, enqueue_embedding_job,
};

/// Maximum raw chunk size for extracted text (256 KiB) — matches the asset
/// chunk size and fits under PostgreSQL toast thresholds comfortably.
const MAX_CHUNK_SIZE: usize = 262_144;

/// Separator inserted between pages when building `full_text`. Byte offsets
/// track the page text itself, not the separator, so searches can map a
/// byte offset back to a single page.
const PAGE_SEPARATOR: &str = "\n\n";

/// PDF text extractor backed by `pdf_oxide`.
pub struct PdfExtractor;

#[async_trait]
impl Extractor for PdfExtractor {
    async fn extract(&self, bytes: Vec<u8>) -> Result<ExtractorOutput, ExtractorError> {
        // `pdf_oxide::PdfDocument` is synchronous and CPU-bound — run it on
        // a blocking thread so we don't stall the runtime on big PDFs.
        tokio::task::spawn_blocking(move || extract_sync(bytes))
            .await
            .map_err(|e| ExtractorError::Internal(format!("join error: {e}")))?
    }
}

fn extract_sync(bytes: Vec<u8>) -> Result<ExtractorOutput, ExtractorError> {
    let doc = pdf_oxide::PdfDocument::from_bytes(bytes)
        .map_err(|e| ExtractorError::Parse(format!("pdf_oxide::from_bytes: {e:?}")))?;

    let page_count = doc
        .page_count()
        .map_err(|e| ExtractorError::Parse(format!("page_count: {e:?}")))?;

    let mut full_text = String::new();
    let mut pages = Vec::with_capacity(page_count);

    for page_idx in 0..page_count {
        let text = doc.extract_text(page_idx).unwrap_or_default();

        let byte_offset = full_text.len();
        let byte_length = text.len();

        pages.push(PageSegment {
            page: page_idx + 1,
            text: text.clone(),
            byte_offset,
            byte_length,
        });

        full_text.push_str(&text);
        if page_idx + 1 < page_count {
            full_text.push_str(PAGE_SEPARATOR);
        }
    }

    Ok(ExtractorOutput { full_text, pages })
}

/// Job handler for the `extract_pdf` background job.
///
/// Returns the extraction id on success (whether newly created or reused
/// from an existing `ok` row). On error returns a string suitable for
/// `mark_background_job_failed` / `mark_asset_extraction_failed`.
pub async fn handle_extract_pdf_job(
    pool: &DbPool,
    job: &BackgroundJobModel,
) -> Result<Uuid, String> {
    let asset_id = job.target_id;

    // --- Idempotency check ----------------------------------------------
    match select_latest_asset_extraction(pool, asset_id, EXTRACTOR_KIND_PDF).await {
        Ok(Some(existing))
            if existing.status == "ok" && existing.extractor_version == EXTRACTOR_VERSION_PDF =>
        {
            log::info!(
                "extract_pdf: asset {} already has ok extraction {}, enqueueing embed",
                asset_id,
                existing.id
            );
            // Make sure downstream embeddings run at least once even if the
            // extraction was reused.
            let _ = enqueue_embedding_job(pool, existing.id).await;
            return Ok(existing.id);
        }
        Ok(_) => {}
        Err(e) => return Err(format!("select_latest_asset_extraction: {e}")),
    }

    // --- Load bytes ------------------------------------------------------
    let bytes = read_asset_content(pool, asset_id)
        .await
        .map_err(|e| format!("read_asset_content({asset_id}): {e}"))?;

    if bytes.is_empty() {
        return Err(format!("asset {asset_id} has no content"));
    }

    // --- Create pending row ---------------------------------------------
    let inserted = insert_asset_extractions(
        pool,
        InsertAssetExtractionArgs {
            asset_ids: vec![asset_id],
            extractor_kinds: vec![EXTRACTOR_KIND_PDF.to_string()],
            extractor_versions: vec![EXTRACTOR_VERSION_PDF],
        },
    )
    .await
    .map_err(|e| format!("insert_asset_extractions: {e}"))?;

    let extraction = inserted
        .into_iter()
        .next()
        .ok_or_else(|| "insert_asset_extractions returned no rows".to_string())?;
    let extraction_id = extraction.id;

    // From here on any failure should mark the extraction as failed.
    let run_result = run_pdf_extraction(pool, asset_id, extraction_id, bytes).await;

    match run_result {
        Ok(()) => Ok(extraction_id),
        Err(err) => {
            if let Err(e) = mark_asset_extraction_failed(pool, extraction_id, &err).await {
                log::warn!(
                    "extract_pdf: mark_asset_extraction_failed({}) failed: {e}",
                    extraction_id
                );
            }
            Err(err)
        }
    }
}

async fn run_pdf_extraction(
    pool: &DbPool,
    asset_id: Uuid,
    extraction_id: Uuid,
    bytes: Vec<u8>,
) -> Result<(), String> {
    // --- Running ---------------------------------------------------------
    mark_asset_extraction_running(pool, extraction_id)
        .await
        .map_err(|e| format!("mark_asset_extraction_running: {e}"))?;

    // --- Extract ---------------------------------------------------------
    let output = PdfExtractor
        .extract(bytes)
        .await
        .map_err(|e| format!("pdf extract: {e}"))?;

    let page_count = output.pages.len();
    let text_length = output.full_text.len() as i64;

    // --- Write raw chunks -----------------------------------------------
    let chunk_ids = if output.full_text.is_empty() {
        Vec::new()
    } else {
        let data_chunks: Vec<Vec<u8>> = output
            .full_text
            .as_bytes()
            .chunks(MAX_CHUNK_SIZE)
            .map(|c| c.to_vec())
            .collect();

        insert_raw_chunks(pool, data_chunks)
            .await
            .map_err(|e| format!("insert_raw_chunks: {e}"))?
    };

    // --- Link chunks to extraction --------------------------------------
    if !chunk_ids.is_empty() {
        let n = chunk_ids.len();
        let args = InsertAssetExtractionChunkArgs {
            extraction_ids: vec![extraction_id; n],
            chunk_ids: chunk_ids.clone(),
            ordinals: (0..n as i32).collect(),
        };
        insert_asset_extraction_chunks(pool, args)
            .await
            .map_err(|e| format!("insert_asset_extraction_chunks: {e}"))?;
    }

    // --- Write positions ------------------------------------------------
    if !output.pages.is_empty() {
        let n = output.pages.len();
        let args = InsertAssetExtractionPositionArgs {
            extraction_ids: vec![extraction_id; n],
            byte_offsets: output.pages.iter().map(|p| p.byte_offset as i64).collect(),
            byte_lengths: output.pages.iter().map(|p| p.byte_length as i64).collect(),
            source_positions: output
                .pages
                .iter()
                .map(|p| json!({ "page": p.page }))
                .collect(),
        };
        insert_asset_extraction_positions(pool, args)
            .await
            .map_err(|e| format!("insert_asset_extraction_positions: {e}"))?;
    }

    // --- Mark ok ---------------------------------------------------------
    let metadata = json!({
        "page_count": page_count,
        "chunk_count": chunk_ids.len(),
    });

    mark_asset_extraction_ok(
        pool,
        extraction_id,
        text_length,
        Some(page_count as i32),
        metadata,
    )
    .await
    .map_err(|e| format!("mark_asset_extraction_ok: {e}"))?;

    log::info!(
        "extract_pdf: asset {} -> extraction {} ({} pages, {} bytes, {} chunks)",
        asset_id,
        extraction_id,
        page_count,
        text_length,
        chunk_ids.len()
    );

    // --- Enqueue chained embedding job ----------------------------------
    if let Err(e) = enqueue_embedding_job(pool, extraction_id).await {
        log::warn!(
            "extract_pdf: enqueue_embedding_job({}) failed: {}",
            extraction_id,
            e
        );
    }

    Ok(())
}
