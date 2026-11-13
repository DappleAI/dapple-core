//! Content extraction workers and job orchestration.
//!
//! This module implements the extractor pipeline described in the Phase B
//! plan of the content pipeline:
//!
//! 1. The API enqueues an `extract_pdf` `BackgroundJob` when a PDF asset is
//!    finalized.
//! 2. `run_extraction_workers` runs a simple polling loop that claims jobs
//!    via `claim_next_background_job` and dispatches them to the right
//!    handler (`handle_extract_pdf_job` / `handle_embed_extraction_job`).
//! 3. On successful PDF extraction the worker enqueues a chained
//!    `embed_extraction` job so the embedding worker can produce vectors.
//!
//! This is an MVP — no LISTEN/NOTIFY, no scheduler integration; just a
//! 1-second idle poll and `tokio::spawn` workers sharing the pool.

use async_trait::async_trait;
use dapple_data::DbPool;
use dapple_data::model::background_jobs::{BackgroundJobModel, InsertBackgroundJobArgs};
use dapple_data::query::asset_extractions::select_latest_asset_extraction;
use dapple_data::query::background_jobs::{
    claim_next_background_job, insert_background_jobs, mark_background_job_failed,
    mark_background_job_ok,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub mod dataset;
pub mod embedding;
pub mod pdf;

pub use dataset::{enqueue_dataset_extraction, handle_extract_dataset_job};
pub use embedding::handle_embed_extraction_job;
pub use pdf::{PdfExtractor, handle_extract_pdf_job};

/// Extractor kind key stored in `rev.asset_extractions.extractor_kind` for PDF
/// text extraction.
pub const EXTRACTOR_KIND_PDF: &str = "pdf";

/// Version of the PDF extractor. Bump this when the extraction algorithm or
/// output format changes so existing extractions are not reused.
pub const EXTRACTOR_VERSION_PDF: i32 = 1;

/// Job kind for the PDF extraction worker.
pub const JOB_KIND_EXTRACT_PDF: &str = "extract_pdf";

/// Version of the dataset (spreadsheet / tabular) extractor. Bump when the
/// envelope format or inference changes so existing extractions are not reused.
pub const EXTRACTOR_VERSION_DATASET: i32 = 1;

/// Job kind for the dataset extraction worker.
pub const JOB_KIND_EXTRACT_DATASET: &str = "extract_dataset";

/// Job kind for the embedding worker (runs after extraction completes).
pub const JOB_KIND_EMBED_EXTRACTION: &str = "embed_extraction";

/// Idle poll interval when there is no work available to claim.
const IDLE_POLL: Duration = Duration::from_secs(1);

// ============================================================================
// Extractor trait
// ============================================================================

/// A page segment of extracted text with its offset into the joined full text.
#[derive(Clone, Debug)]
pub struct PageSegment {
    /// 1-based page number.
    pub page: usize,
    /// Plain text for this page (not including the join separator).
    pub text: String,
    /// Byte offset of this page's text within `ExtractorOutput::full_text`.
    pub byte_offset: usize,
    /// Byte length of this page's text within `ExtractorOutput::full_text`.
    pub byte_length: usize,
}

/// Output of an extractor run — a concatenated full-text string plus
/// per-page offsets into it.
#[derive(Clone, Debug)]
pub struct ExtractorOutput {
    pub full_text: String,
    pub pages: Vec<PageSegment>,
}

/// Trait implemented by each concrete extractor (PDF today, DOCX / HTML /
/// image OCR later). Must be `Send + Sync` so workers can hold trait objects
/// across `.await` points.
#[async_trait]
pub trait Extractor: Send + Sync {
    async fn extract(&self, bytes: Vec<u8>) -> Result<ExtractorOutput, ExtractorError>;
}

/// Errors produced by extractors.
#[derive(Debug, thiserror::Error)]
pub enum ExtractorError {
    #[error("parse failure: {0}")]
    Parse(String),
    #[error("io failure: {0}")]
    Io(String),
    #[error("internal extractor error: {0}")]
    Internal(String),
}

// ============================================================================
// Job enqueue helpers
// ============================================================================

/// Enqueue a PDF extraction job for the given asset.
///
/// Idempotent-ish: if an `ok` extraction already exists for this asset at
/// the current extractor version we skip enqueueing so re-finalizing the
/// same asset doesn't create duplicate work.
pub async fn enqueue_pdf_extraction(pool: &DbPool, asset_id: Uuid) -> Result<Option<Uuid>, String> {
    // Short-circuit if we already have an ok extraction at this version.
    match select_latest_asset_extraction(pool, asset_id, EXTRACTOR_KIND_PDF).await {
        Ok(Some(existing))
            if existing.status == "ok" && existing.extractor_version == EXTRACTOR_VERSION_PDF =>
        {
            log::debug!(
                "enqueue_pdf_extraction: skipping asset {} (extraction {} already ok)",
                asset_id,
                existing.id
            );
            return Ok(None);
        }
        Ok(_) => {}
        Err(e) => {
            return Err(format!("lookup existing extraction: {e}"));
        }
    }

    let args = InsertBackgroundJobArgs {
        job_kinds: vec![JOB_KIND_EXTRACT_PDF.to_string()],
        target_ids: vec![asset_id],
        payloads: vec![json!({
            "extractor_kind": EXTRACTOR_KIND_PDF,
            "extractor_version": EXTRACTOR_VERSION_PDF,
        })],
    };

    let jobs = insert_background_jobs(pool, args)
        .await
        .map_err(|e| format!("insert extract_pdf job: {e}"))?;

    Ok(jobs.into_iter().next().map(|j| j.id))
}

/// Enqueue an embedding job for a completed extraction. `target_id` is the
/// extraction id, not the asset id — the embedding worker resolves the
/// asset transitively.
pub async fn enqueue_embedding_job(
    pool: &DbPool,
    extraction_id: Uuid,
) -> Result<Option<Uuid>, String> {
    let args = InsertBackgroundJobArgs {
        job_kinds: vec![JOB_KIND_EMBED_EXTRACTION.to_string()],
        target_ids: vec![extraction_id],
        payloads: vec![json!({})],
    };

    let jobs = insert_background_jobs(pool, args)
        .await
        .map_err(|e| format!("insert embed_extraction job: {e}"))?;

    Ok(jobs.into_iter().next().map(|j| j.id))
}

// ============================================================================
// Worker loop
// ============================================================================

/// Spawn `concurrency` parallel worker tasks and drive them forever.
///
/// Each worker polls `claim_next_background_job` with the set of kinds we
/// handle. On a claim it dispatches to the right handler, marks ok or
/// failed, and loops. When idle it sleeps `IDLE_POLL` before trying again.
///
/// This function never returns under normal operation — await it from a
/// `tokio::spawn` in your process's `main()`.
pub async fn run_extraction_workers(
    pool: Arc<DbPool>,
    concurrency: usize,
    conduit_url: Option<String>,
) {
    let concurrency = concurrency.max(1);
    log::info!(
        "dapple-prompt-flow extraction workers starting (concurrency={}, conduit={})",
        concurrency,
        conduit_url.as_deref().unwrap_or("<unset>")
    );

    let mut handles = Vec::with_capacity(concurrency);
    for i in 0..concurrency {
        let pool = pool.clone();
        let conduit_url = conduit_url.clone();
        handles.push(tokio::spawn(async move {
            worker_loop(i, pool, conduit_url).await;
        }));
    }

    // Await all worker tasks — if any panics we log and keep the rest
    // running rather than aborting the process.
    for (i, h) in handles.into_iter().enumerate() {
        if let Err(e) = h.await {
            log::error!("extraction worker {i} join error: {e}");
        }
    }
}

async fn worker_loop(worker_id: usize, pool: Arc<DbPool>, conduit_url: Option<String>) {
    let kinds = vec![
        JOB_KIND_EXTRACT_PDF.to_string(),
        JOB_KIND_EXTRACT_DATASET.to_string(),
        JOB_KIND_EMBED_EXTRACTION.to_string(),
    ];

    loop {
        let job = match claim_next_background_job(pool.as_ref(), &kinds).await {
            Ok(Some(job)) => job,
            Ok(None) => {
                tokio::time::sleep(IDLE_POLL).await;
                continue;
            }
            Err(e) => {
                log::warn!(
                    "worker {worker_id}: claim_next_background_job failed: {e}; backing off"
                );
                tokio::time::sleep(IDLE_POLL).await;
                continue;
            }
        };

        log::info!(
            "worker {worker_id}: claimed job id={} kind={} target={}",
            job.id,
            job.job_kind,
            job.target_id
        );

        let result = dispatch_job(pool.as_ref(), conduit_url.as_deref(), &job).await;
        match result {
            Ok(()) => {
                if let Err(e) = mark_background_job_ok(pool.as_ref(), job.id).await {
                    log::warn!(
                        "worker {worker_id}: mark_background_job_ok({}) failed: {e}",
                        job.id
                    );
                }
                log::info!("worker {worker_id}: job {} completed ok", job.id);
            }
            Err(err) => {
                log::warn!("worker {worker_id}: job {} failed: {err}", job.id);
                if let Err(e) = mark_background_job_failed(pool.as_ref(), job.id, &err).await {
                    log::warn!(
                        "worker {worker_id}: mark_background_job_failed({}) failed: {e}",
                        job.id
                    );
                }
            }
        }
    }
}

async fn dispatch_job(
    pool: &DbPool,
    conduit_url: Option<&str>,
    job: &BackgroundJobModel,
) -> Result<(), String> {
    match job.job_kind.as_str() {
        JOB_KIND_EXTRACT_PDF => handle_extract_pdf_job(pool, job).await.map(|_| ()),
        JOB_KIND_EXTRACT_DATASET => handle_extract_dataset_job(pool, job).await.map(|_| ()),
        JOB_KIND_EMBED_EXTRACTION => handle_embed_extraction_job(pool, job, conduit_url).await,
        other => Err(format!("unknown job kind: {other}")),
    }
}
